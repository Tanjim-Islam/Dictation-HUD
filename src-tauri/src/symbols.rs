/// Symbol replacement layer between STT and AI refinement.
/// Converts spoken symbol names to actual symbols before AI processing.

/// Symbol mappings: (spoken phrase, replacement symbol)
/// Case-insensitive matching is applied.
pub const SYMBOL_MAPPINGS: &[(&str, &str)] = &[
    // Line breaks
    ("new line", "\n"),
    ("newline", "\n"),
    ("line break", "\n"),
    ("next line", "\n"),
    ("enter", "\n"),
    ("new paragraph", "\n\n"),
    ("paragraph break", "\n\n"),
    
    // Dashes
    ("em dash", "—"),
    ("emdash", "—"),
    ("m-dash", "—"),
    ("m dash", "—"),
    ("en dash", "–"),
    ("endash", "–"),
    ("n-dash", "–"),
    ("n dash", "–"),
    ("dash", "-"),
    ("hyphen", "-"),
    
    // Punctuation (for users who say them explicitly)
    ("full stop", "."),
    ("period", "."),
    ("dot", "."),
    ("comma", ","),
    ("colon", ":"),
    ("semicolon", ";"),
    ("semi colon", ";"),
    ("question mark", "?"),
    ("exclamation mark", "!"),
    ("exclamation point", "!"),
    ("ellipsis", "..."),
    ("triple dot", "..."),
    
    // Quotes and brackets
    ("open quote", "\""),
    ("close quote", "\""),
    ("open single quote", "'"),
    ("close single quote", "'"),
    ("open paren", "("),
    ("close paren", ")"),
    ("open parenthesis", "("),
    ("close parenthesis", ")"),
    ("open parentheses", "("),
    ("close parentheses", ")"),
    ("open bracket", "["),
    ("close bracket", "]"),
    ("open brace", "{"),
    ("close brace", "}"),
    ("open curly", "{"),
    ("close curly", "}"),
    
    // Math and programming symbols
    ("plus sign", "+"),
    ("plus", "+"),
    ("minus sign", "-"),
    ("minus", "-"),
    ("equals sign", "="),
    ("equals", "="),
    ("equal sign", "="),
    ("equal", "="),
    ("asterisk", "*"),
    ("star", "*"),
    ("forward slash", "/"),
    ("slash", "/"),
    ("backslash", "\\"),
    ("back slash", "\\"),
    ("percent sign", "%"),
    ("percent", "%"),
    ("ampersand", "&"),
    ("and sign", "&"),
    ("at sign", "@"),
    ("at symbol", "@"),
    ("hashtag", "#"),
    ("hash", "#"),
    ("pound sign", "#"),
    ("dollar sign", "$"),
    ("dollar", "$"),
    ("caret", "^"),
    ("underscore", "_"),
    ("pipe", "|"),
    ("vertical bar", "|"),
    ("tilde", "~"),
    ("backtick", "`"),
    ("grave accent", "`"),
    
    // Comparison
    ("less than", "<"),
    ("greater than", ">"),
    ("less than or equal", "<="),
    ("greater than or equal", ">="),
    ("not equal", "!="),
    
    // Arrows (common in notes)
    ("right arrow", "→"),
    ("left arrow", "←"),
    ("up arrow", "↑"),
    ("down arrow", "↓"),
    
    // Common symbols
    ("bullet point", "•"),
    ("bullet", "•"),
    ("degree sign", "°"),
    ("degree", "°"),
    ("copyright", "©"),
    ("registered", "®"),
    ("trademark", "™"),
];

/// Replace spoken symbol names with actual symbols.
/// Processes longer phrases first to avoid partial matches.
pub fn replace_symbols(text: &str) -> String {
    let mut result = text.to_string();
    
    // Sort by length descending so longer phrases match first
    let mut mappings: Vec<_> = SYMBOL_MAPPINGS.iter().collect();
    mappings.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    
    for (spoken, symbol) in mappings {
        let pattern = spoken.to_lowercase();
        let mut new_result = String::new();
        let mut remaining = result.as_str();
        
        while !remaining.is_empty() {
            if let Some(pos) = remaining.to_lowercase().find(&pattern) {
                // Check word boundaries
                let before_ok = pos == 0 || 
                    !remaining.chars().nth(pos - 1).map(|c| c.is_alphanumeric()).unwrap_or(false);
                let after_pos = pos + spoken.len();
                let after_ok = after_pos >= remaining.len() ||
                    !remaining[after_pos..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false);
                
                if before_ok && after_ok {
                    // For newlines, trim surrounding spaces AND punctuation
                    if symbol.contains('\n') {
                        let before = remaining[..pos].trim_end_matches(|c| c == ' ' || c == ',');
                        new_result.push_str(before);
                        new_result.push_str(symbol);
                        remaining = remaining[after_pos..].trim_start_matches(|c: char| c == ' ' || c == ',' || c == '.');
                    } else {
                        // For punctuation symbols, trim the comma/space before but keep space after
                        let before = remaining[..pos].trim_end_matches(|c| c == ' ' || c == ',');
                        new_result.push_str(before);
                        new_result.push_str(symbol);
                        // Only trim the comma after, keep the space
                        remaining = remaining[after_pos..].trim_start_matches(',');
                    }
                } else {
                    // Not a word boundary match, skip past this occurrence
                    new_result.push_str(&remaining[..pos + 1]);
                    remaining = &remaining[pos + 1..];
                }
            } else {
                new_result.push_str(remaining);
                break;
            }
        }
        result = new_result;
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_replacements() {
        assert_eq!(replace_symbols("hello new line world"), "hello\nworld");
        assert_eq!(replace_symbols("test em dash here"), "test— here");
        assert_eq!(replace_symbols("add hashtag symbol"), "add# symbol");
    }
    
    #[test]
    fn test_case_insensitive() {
        assert_eq!(replace_symbols("Hello NEW LINE World"), "Hello\nWorld");
        assert_eq!(replace_symbols("EM DASH"), "—");
    }
    
    #[test]
    fn test_multiple_symbols() {
        assert_eq!(
            replace_symbols("line one new line line two new line line three"),
            "line one\nline two\nline three"
        );
    }
    
    #[test]
    fn test_comma_trimming() {
        // ElevenLabs adds commas around symbol words
        assert_eq!(
            replace_symbols("Dear John, New line, New line, I wanted to tell you"),
            "Dear John\n\nI wanted to tell you"
        );
        assert_eq!(
            replace_symbols("This is important, Exclamation mark, Please call"),
            "This is important! Please call"
        );
    }
}
