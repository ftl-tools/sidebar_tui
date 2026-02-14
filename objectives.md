# Hot Rod CLI

I want a simple TUI for managing terminal sessions in a sidebar in a way that works with my workflow. I want to have sessions grouped into threads on the side bar, and to be able to easily create new ones and switch between them. The full spec is here @eventual_objectives.md but that is too much to start with, so we'll get the basic functionality working first and then iterate from there.

## Basic Objectives

- You MUST use TypeScript, commander, and OpenTUI. Research and use any other tools that are needed to accomplish the objectives.
- Running `hr` should open the TUI.
- On the Left side should be a fixed-width 20-character sidebar.
  - The top row of the sidebar should be blue and contain the title "Hot Rod" centered in it. The text in this first row should be black.
  - The rest of the sidebar should be empty for now, but it should have a lighter background color than the terminal view.
- The right side should be the terminal view, which should take up the rest of the space.
  - The terminal view should show a fully functional terminal where I can run commands and see their output. Or even run command line applications like vi.
  - The terminal should open to the current working directory of the terminal where I ran `hr`.
- Running `ctrl + q` should quit the TUI and return me to my normal terminal.
- You must create at the very least the following E2E tests. They must all work in the Apple terminal on my computer:
  - You have used the Hot Rod TUI and it's layout matches the spec above exactly.
  - You can run `git status` in the TUI terminal and it has the same output as running `git status` in a normal terminal.
  - You can run `vi` in the TUI terminal, open a file, edit it, save it, and exit, and the file is changed as expected.
  - In the TUI terminal, you can start typing `git status`, backspace before you send it, type `echo "hello world"`, send that, and see the expected output in the TUI terminal.
- NO E2E or unit tests are skipped, FOR ANY REASON. Install requirements if missing, or do WHATEVER IS REQUIRED to unblock them.
- The cli had been built and linked globally.
