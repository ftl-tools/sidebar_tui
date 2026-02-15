# Separate Socket Channels Research Spike

## Research Issue: sidebar_tui-hp0

## Problem Summary

The live preview feature needs to send Preview messages asynchronously during sidebar navigation. The current architecture uses a single Unix socket connection for both streaming Output and synchronous request-response operations (CreateSession, DeleteSession, Attach, etc.). This creates a message interleaving problem when async and sync operations mix on the same connection.

## Current Architecture

### Single Socket Pattern
```
┌─────────────────────────────────────────────────────────────────┐
│                        Single UnixStream                         │
│  ┌─────────┐         ┌────────────────┐         ┌─────────┐     │
│  │ Client  │ ──────► │ handle_client  │ ◄────── │ Session │     │
│  │ TUI     │ ◄────── │ (daemon.rs)    │ ──────► │ PTY     │     │
│  └─────────┘         └────────────────┘         └─────────┘     │
│                                                                  │
│  Messages: Input, Attach, Detach, Preview, CreateSession, etc.  │
│  Responses: Output, Attached, Previewed, Created, etc.          │
└─────────────────────────────────────────────────────────────────┘
```

### The Problem
- `MessageReader` buffers data in the async event loop
- Sync operations use `decode_message()` with `read_exact()` directly on socket
- If Preview responses are in flight when a sync op starts, messages get interleaved
- Result: JSON deserialization errors or wrong message types

## Separate Sockets Approach

### Proposed Pattern
```
┌───────────────────────────────────────────────────────────────────┐
│                     Dual Socket Connections                        │
│                                                                    │
│  ┌─────────┐   Stream Socket    ┌────────────────┐   ┌─────────┐  │
│  │         │ ←───────────────── │                │ ← │ Session │  │
│  │ Client  │                    │ handle_client  │   │ PTY     │  │
│  │ TUI     │   Control Socket   │ (daemon.rs)    │   │         │  │
│  │         │ ──────────────────►│                │   │         │  │
│  │         │ ◄─────────────────│                │   │         │  │
│  └─────────┘                    └────────────────┘   └─────────┘  │
│                                                                    │
│  Stream: Output, Previewed (async, push-based)                    │
│  Control: Attach, CreateSession, DeleteSession, etc (sync req/res)│
└───────────────────────────────────────────────────────────────────┘
```

### How Zellij Handles This

Zellij uses **internal channels** (crossbeam mpsc) rather than multiple sockets:
- `ThreadSenders` distributes messages to different components
- `SenderWithContext<ScreenInstruction>`, `SenderWithContext<PtyInstruction>`, etc.
- For web clients, they use separate WebSocket channels: `control_channel_tx` and `terminal_channel_tx`

However, for Unix socket IPC, Zellij still uses a single connection but with a more sophisticated message routing architecture.

## Implementation Analysis

### Option A: True Dual Sockets

Connect twice to the daemon, establish client identity coordination:

```rust
pub struct DaemonClient {
    stream_socket: UnixStream,   // For Output/Preview responses (async)
    control_socket: UnixStream,  // For sync request/response ops
    client_id: Uuid,
}

impl DaemonClient {
    pub fn connect() -> Result<Self> {
        let socket_path = get_socket_path();

        // First connection: control channel
        let control_socket = UnixStream::connect(&socket_path)?;

        // Get client ID from initial handshake
        let handshake = encode_message(&ClientMessage::Handshake)?;
        control_socket.write_all(&handshake)?;
        let response = decode_message(&control_socket)?;
        let client_id = match response {
            DaemonResponse::HandshakeOk { client_id } => client_id,
            _ => bail!("Unexpected handshake response"),
        };

        // Second connection: stream channel
        let stream_socket = UnixStream::connect(&socket_path)?;
        let join = encode_message(&ClientMessage::JoinStream { client_id })?;
        stream_socket.write_all(&join)?;
        // Stream socket doesn't need sync responses
        stream_socket.set_nonblocking(true)?;

        Ok(Self { stream_socket, control_socket, client_id })
    }
}
```

Daemon side changes:
```rust
fn handle_client(
    mut control_stream: UnixStream,
    sessions: Arc<Mutex<HashMap<String, Session>>>,
    shutdown: Arc<AtomicBool>,
    stream_sockets: Arc<Mutex<HashMap<Uuid, UnixStream>>>,
) -> Result<()> {
    // Wait for handshake
    let msg = read_message(&mut control_stream)?;
    let client_id = match msg {
        ClientMessage::Handshake => {
            let id = Uuid::new_v4();
            send_response(&mut control_stream, &DaemonResponse::HandshakeOk { client_id: id })?;
            id
        }
        ClientMessage::JoinStream { client_id } => {
            // This is a stream connection for an existing client
            stream_sockets.lock().unwrap().insert(client_id, control_stream);
            return Ok(());  // Stream handler exits - main loop pushes to this socket
        }
        _ => bail!("Expected handshake"),
    };

    // Normal control message handling continues...
}
```

### Option B: Single Socket with Internal Channel Separation

Keep single Unix socket but use internal mpsc channels like Zellij:

```rust
// Client side
pub struct DaemonClient {
    stream: UnixStream,
    output_rx: Receiver<DaemonResponse>,  // Async output
    response_rx: Receiver<DaemonResponse>, // Sync responses
}

// Background thread reads from socket and routes
fn socket_reader(
    stream: UnixStream,
    output_tx: Sender<DaemonResponse>,
    response_tx: Sender<DaemonResponse>,
) {
    loop {
        let msg = decode_message(&stream)?;
        match msg {
            DaemonResponse::Output { .. } | DaemonResponse::Previewed { .. } => {
                output_tx.send(msg)?;
            }
            _ => {
                response_tx.send(msg)?;
            }
        }
    }
}
```

## Complexity Assessment

### Option A: True Dual Sockets
| Aspect | Assessment |
|--------|------------|
| Daemon changes | **High** - Need client ID tracking, stream socket storage, coordination |
| Client changes | **Medium** - Two connections, handshake protocol |
| Message routing | **Low** - Clean separation at socket level |
| Resource overhead | **Medium** - 2x file descriptors per client |
| Error handling | **High** - Must handle either socket dying independently |
| Testing | **High** - More connection states to test |

### Option B: Internal Channels
| Aspect | Assessment |
|--------|------------|
| Daemon changes | **None** - Stays single socket |
| Client changes | **Medium** - Add background reader thread + channels |
| Message routing | **Medium** - Need to categorize messages |
| Resource overhead | **Low** - Just threads and channels |
| Error handling | **Low** - Single connection point of failure |
| Testing | **Low** - Existing infrastructure works |

## Comparison with Hybrid Sync (sidebar_tui-dhj)

| Approach | Complexity | Risk | Eliminates Interleaving? |
|----------|------------|------|--------------------------|
| Dual Sockets | High | Medium | Yes (complete) |
| Internal Channels (B) | Medium | Low | Yes (at routing layer) |
| Hybrid Drain-Before-Sync | Low | Low | Yes (with timeout) |

## Key Questions Answered

### Would dual sockets completely eliminate the interleaving problem?
**Yes** - If stream data flows only on the stream socket and control requests only on the control socket, there's no possibility of interleaving.

### How complex would the refactor be?
**High for Option A, Medium for Option B** - Option A requires:
1. New handshake protocol
2. Client ID tracking in daemon
3. Stream socket registry
4. Coordination of socket cleanup when either dies
5. New tests for all the connection edge cases

Option B requires:
1. Background reader thread
2. Two mpsc channels
3. Message categorization logic

### Are there examples of dual-socket patterns in references/?
**Not directly** - Zellij uses WebSockets for web clients (naturally dual-channel via `terminal_ws` and `control_ws`), but for Unix sockets they use internal channels instead. The web client pattern shows the concept works, just implemented at a different layer.

## Recommendation

**Option B (Internal Channels) is preferred over dual sockets** because:
1. No daemon protocol changes required
2. Simpler error handling (single connection point)
3. Matches Zellij's proven pattern for handling this
4. Can be done incrementally without breaking existing clients

However, compared to the **Hybrid Drain-Before-Sync** approach (sidebar_tui-dhj):
- Internal channels is cleaner but requires more code
- Drain-before-sync is simpler (~30 lines) but has edge cases
- For just the Preview feature, drain-before-sync may be sufficient

## Files Analyzed

- `src/daemon.rs`: DaemonClient, handle_client, socket handling
- `src/main.rs`: Client connection and message loop
- `references/existing-projects/zellij/zellij-client/src/lib.rs`: Channel patterns
- `references/existing-projects/zellij/zellij-server/src/thread_bus.rs`: ThreadSenders pattern
- `references/existing-projects/zellij/zellij-client/src/web_client/types.rs`: Dual-channel web client
- `references/existing-projects/zellij/zellij-client/src/web_client/connection_manager.rs`: Channel routing
