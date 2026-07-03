//! Shared tokenizer for the corrections engine.
//!
//! CRITICAL INVARIANT: this tokenizer is the ONLY tokenizer used by both
//! correction-pair extraction (the E1 "fix last transcript" diff) and the
//! apply pass. Extraction and apply MUST tokenize identically, or learned
//! pairs silently never fire (e.g. a pair learned with an attached trailing
//! comma would never match clean text). Do not introduce a second
//! tokenization scheme anywhere in the corrections pipeline.
//!
//!   text: «Abre el PR, don't wait — está listo (¡ya!)»
//!          └──┘ └┘ └┘  └───┘ └──┘   └──┘ └───┘   └┘
//!          tokens = maximal runs of Unicode alphanumerics,
//!          plus apostrophes BETWEEN alphanumerics ("don't" is one token)
//!
//! Normalization for matching: NFC → Unicode lowercase. This makes matching
//! diacritic-correct (Á/á, Ñ/ñ) and immune to NFD input (macOS text fields
//! can produce decomposed "n + combining tilde").

use unicode_normalization::UnicodeNormalization;

/// A token's byte range within the original text. `start..end` always lies on
/// UTF-8 character boundaries of the source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
}

impl Token {
    /// The token's text slice within `source`.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// True for characters that form the body of a token.
///
/// Combining marks (e.g. U+0301 COMBINING ACUTE) are word chars: NFD input
/// like "cafe\u{0301}" must tokenize as ONE token so normalization (NFC +
/// lowercase) can match it against precomposed "café".
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || unicode_normalization::char::is_combining_mark(c)
}

/// True for apostrophe variants that may join two word chars ("don't", "l'appel").
fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// Tokenize `text` into byte-ranged tokens.
///
/// A token is a maximal run of Unicode alphanumerics, where an apostrophe is
/// included only when it sits directly between two alphanumerics. Everything
/// else (whitespace, punctuation incl. ¿ ¡ — , .) separates tokens and is
/// preserved verbatim by the apply pass via the byte ranges.
pub fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let (start, c) = chars[i];
        if !is_word_char(c) {
            i += 1;
            continue;
        }
        // Consume a token: word chars, plus apostrophes flanked by word chars.
        let mut j = i + 1;
        while j < chars.len() {
            let (_, cj) = chars[j];
            if is_word_char(cj) {
                j += 1;
            } else if is_apostrophe(cj) && j + 1 < chars.len() && is_word_char(chars[j + 1].1) {
                j += 2; // apostrophe + following word char are both part of the token
            } else {
                break;
            }
        }
        let end = if j < chars.len() {
            chars[j].0
        } else {
            text.len()
        };
        tokens.push(Token { start, end });
        i = j;
    }

    tokens
}

/// Normalize a token's text for matching: NFC, then Unicode lowercase.
pub fn normalize(token_text: &str) -> String {
    token_text.nfc().collect::<String>().to_lowercase()
}

/// Tokenize and normalize in one step. This is the canonical "matching key"
/// form of a phrase — used for pattern parsing, dedup, and extraction.
pub fn normalized_tokens(text: &str) -> Vec<String> {
    tokenize(text)
        .iter()
        .map(|t| normalize(t.text(text)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(text: &str) -> Vec<&str> {
        tokenize(text).iter().map(|t| t.text(text)).collect()
    }

    #[test]
    fn splits_on_whitespace_and_punctuation() {
        assert_eq!(toks("abre el PR, ya"), vec!["abre", "el", "PR", "ya"]);
    }

    #[test]
    fn spanish_inverted_punctuation_separates() {
        assert_eq!(toks("¿voy a push? ¡sí!"), vec!["voy", "a", "push", "sí"]);
    }

    #[test]
    fn apostrophe_between_letters_is_one_token() {
        assert_eq!(toks("don't wait"), vec!["don't", "wait"]);
        // Curly apostrophe too
        assert_eq!(toks("don\u{2019}t"), vec!["don\u{2019}t"]);
    }

    #[test]
    fn leading_or_trailing_apostrophe_is_not_part_of_token() {
        assert_eq!(toks("'hello' world"), vec!["hello", "world"]);
    }

    #[test]
    fn diacritics_are_word_chars() {
        assert_eq!(toks("está el niño"), vec!["está", "el", "niño"]);
    }

    #[test]
    fn numbers_are_tokens() {
        assert_eq!(toks("GPT4 y v2.0"), vec!["GPT4", "y", "v2", "0"]);
    }

    #[test]
    fn hyphen_splits_tokens() {
        assert_eq!(toks("code-switching"), vec!["code", "switching"]);
    }

    #[test]
    fn empty_and_punctuation_only() {
        assert!(toks("").is_empty());
        assert!(toks("... ,, ¿? —").is_empty());
    }

    #[test]
    fn normalize_folds_case_and_diacritics_survive() {
        assert_eq!(normalize("ESTÁ"), "está");
        assert_eq!(normalize("Ñoño"), "ñoño");
        assert_eq!(normalize("Café"), "café");
    }

    #[test]
    fn normalize_nfd_equals_nfc() {
        // "café" with decomposed e + combining acute vs precomposed é
        let nfd = "cafe\u{0301}";
        let nfc = "caf\u{00e9}";
        assert_eq!(normalize(nfd), normalize(nfc));
    }

    #[test]
    fn token_ranges_are_exact() {
        let text = "hola, mundo";
        let tokens = tokenize(text);
        assert_eq!(tokens[0].text(text), "hola");
        assert_eq!(tokens[1].text(text), "mundo");
        assert_eq!(&text[tokens[0].end..tokens[1].start], ", ");
    }
}
