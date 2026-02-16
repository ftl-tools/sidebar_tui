//! Random session name generator using three 3-4 letter words.
//!
//! Generates memorable session names like "Fox blue moon" by combining
//! three random short words. The first word is capitalized.

use rand::seq::SliceRandom;
use rand::Rng;

/// List of common 3-4 letter words suitable for session names.
/// These are simple, memorable words that create readable combinations.
const WORDS: &[&str] = &[
    // Nature
    "sun", "moon", "star", "tree", "leaf", "rain", "snow", "wind", "fire", "lake",
    "rock", "sand", "wave", "sky", "dew", "fog", "ice", "sea", "bay", "hill",
    // Animals
    "fox", "owl", "bear", "wolf", "deer", "fish", "bird", "hawk", "bee", "ant",
    "cat", "dog", "bat", "elk", "eel", "ram", "hen", "jay", "cod", "cow",
    // Colors
    "red", "blue", "gold", "jade", "gray", "pink", "tan", "teal", "navy", "lime",
    // Objects
    "cup", "box", "key", "pen", "pin", "jar", "bag", "hat", "map", "gem",
    "bell", "book", "door", "lamp", "ring", "rope", "tent", "vase", "wick", "yarn",
    // Actions/States
    "calm", "bold", "cool", "fast", "free", "glad", "keen", "kind", "safe", "warm",
    "wild", "wise", "neat", "pure", "rare", "soft", "tall", "true", "vast", "wide",
    // Time
    "dawn", "dusk", "noon", "eve", "fall", "rise",
    // Misc short words
    "ace", "arc", "arm", "art", "ash", "axe", "bud", "cap", "cog", "cub",
    "den", "dot", "elm", "eye", "fin", "fur", "gap", "gum", "hay", "hut",
    "ink", "ivy", "jet", "jot", "kit", "lap", "leg", "lip", "log", "lot",
    "mat", "mix", "mud", "net", "nod", "nut", "oak", "oar", "orb", "ore",
    "pad", "paw", "pea", "pit", "pod", "pot", "pub", "rag", "ray", "rib",
    "rod", "row", "rub", "rug", "rye", "sap", "saw", "sip", "sob", "sod",
    "spa", "sum", "tab", "tag", "tap", "tar", "tin", "tip", "toe", "top",
    "tub", "tug", "van", "vat", "vet", "vim", "wax", "web", "wig", "wit",
    "yak", "yam", "zap", "zen", "zip",
];

/// Generate a random session name from three 3-4 letter words.
/// The first letter of the first word is capitalized, rest are lowercase.
/// Words are separated by spaces.
///
/// # Example
/// ```
/// use sidebar_tui::name_generator::generate_session_name;
/// let name = generate_session_name();
/// // e.g., "Fox blue moon"
/// ```
pub fn generate_session_name() -> String {
    let mut rng = rand::thread_rng();
    generate_session_name_with_rng(&mut rng)
}

/// Generate a session name using a specific RNG (for testing).
pub fn generate_session_name_with_rng<R: Rng>(rng: &mut R) -> String {
    let words: Vec<&&str> = WORDS.choose_multiple(rng, 3).collect();

    let mut name = String::new();
    for (i, word) in words.iter().enumerate() {
        if i == 0 {
            // Capitalize first word
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                name.push(first.to_ascii_uppercase());
                name.push_str(chars.as_str());
            }
        } else {
            name.push(' ');
            name.push_str(word);
        }
    }

    name
}

/// Check if a name already exists in a list of existing session names.
/// Returns true if the name exists (case-insensitive comparison).
pub fn name_exists(name: &str, existing_names: &[&str]) -> bool {
    let name_lower = name.to_lowercase();
    existing_names.iter().any(|n| n.to_lowercase() == name_lower)
}

/// Generate a unique session name that doesn't conflict with existing sessions.
/// Tries multiple times to find a unique name before giving up and appending a number.
pub fn generate_unique_session_name(existing_names: &[&str]) -> String {
    let mut rng = rand::thread_rng();

    // Try up to 10 times to generate a unique name
    for _ in 0..10 {
        let name = generate_session_name_with_rng(&mut rng);
        if !name_exists(&name, existing_names) {
            return name;
        }
    }

    // Fallback: append a random number to make it unique
    let base_name = generate_session_name_with_rng(&mut rng);
    let suffix: u16 = rng.gen_range(100..1000);
    format!("{} {}", base_name, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn test_generate_session_name_format() {
        let name = generate_session_name();

        // Should contain exactly 2 spaces (3 words)
        assert_eq!(name.matches(' ').count(), 2, "Name should have 3 words: {}", name);

        // First character should be uppercase
        assert!(name.chars().next().unwrap().is_uppercase(),
                "First letter should be uppercase: {}", name);

        // All characters should be valid
        assert!(name.chars().all(|c| c.is_ascii_alphabetic() || c == ' '),
                "Name should only contain letters and spaces: {}", name);
    }

    #[test]
    fn test_generate_session_name_deterministic_with_seed() {
        let mut rng1 = StdRng::seed_from_u64(12345);
        let mut rng2 = StdRng::seed_from_u64(12345);

        let name1 = generate_session_name_with_rng(&mut rng1);
        let name2 = generate_session_name_with_rng(&mut rng2);

        assert_eq!(name1, name2, "Same seed should produce same name");
    }

    #[test]
    fn test_generate_session_name_variability() {
        // Generate many names and check they're not all the same
        let names: Vec<String> = (0..10).map(|_| generate_session_name()).collect();
        let unique_names: std::collections::HashSet<_> = names.iter().collect();

        // With random generation, we should get mostly unique names
        assert!(unique_names.len() > 1, "Should generate varied names");
    }

    #[test]
    fn test_name_exists_case_insensitive() {
        let existing = vec!["Fox blue moon", "Bear red sun"];

        assert!(name_exists("Fox blue moon", &existing));
        assert!(name_exists("fox blue moon", &existing));
        assert!(name_exists("FOX BLUE MOON", &existing));
        assert!(!name_exists("Cat green star", &existing));
    }

    #[test]
    fn test_generate_unique_session_name_avoids_existing() {
        let existing = vec!["test1", "test2"];
        let name = generate_unique_session_name(&existing);

        // The generated name should not be in existing list
        assert!(!existing.contains(&name.as_str()));
    }

    #[test]
    fn test_words_are_valid_length() {
        for word in WORDS {
            assert!(word.len() >= 3 && word.len() <= 4,
                    "Word '{}' should be 3-4 characters", word);
            assert!(word.chars().all(|c| c.is_ascii_lowercase()),
                    "Word '{}' should be all lowercase", word);
        }
    }

    #[test]
    fn test_word_list_has_enough_variety() {
        // With 3 words chosen from the list, we need enough for good variety
        assert!(WORDS.len() >= 50, "Should have at least 50 words for variety");
    }
}
