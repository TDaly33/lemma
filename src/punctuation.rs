// Trailing/leading punctuation index variants (Feature 2 of the lookup-
// coverage spec). The Kindle reading engine's word-selection can include
// adjacent punctuation in the string handed to the dictionary lookup index,
// and Kindle does not auto-strip it or auto-generate variants, so "λέξη,"
// fails exact lookup even though "λέξη" is indexed. This module generates
// the punctuation-attached forms for a bounded set of forms per headword;
// scoping (which forms get punctuation variants) lives in
// html_gen::entry_variations.
//
// The mark lists below are the spec's starting/default set. They have not
// been validated against a frequency count of punctuation actually adjacent
// to word boundaries in a representative Modern Greek corpus sample - the
// spec calls that out as follow-up work before finalizing the shipped list.

/// Marks that can trail a word: comma, period, ano teleia (Greek "semicolon"
/// point), Greek question mark, colon, closing guillemet/paren/quotes, and
/// the three dash forms.
pub const TRAILING: &[&str] = &[
    ",", ".", "\u{0387}", "\u{037E}", ":", "\u{00BB}", ")", "\"", "'",
    "\u{2014}", "\u{2013}", "-",
];

/// Marks that can lead a word: opening guillemet/paren/quotes and the two
/// (non-hyphen) dash forms.
pub const LEADING: &[&str] = &["\u{00AB}", "(", "\"", "'", "\u{2014}", "\u{2013}"];

/// All `{form}{trailing}` and `{leading}{form}` combinations for one form.
pub fn variants_for(form: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(TRAILING.len() + LEADING.len());
    for p in TRAILING {
        out.push(format!("{}{}", form, p));
    }
    for p in LEADING {
        out.push(format!("{}{}", p, form));
    }
    out
}

/// Closing quote/bracket marks that, per Greek typographic convention, sit
/// immediately before the enclosing sentence's own trailing punctuation
/// with no space - e.g. the guillemet in "παλιόπαιδο»," (the quotation
/// closes, and the sentence's comma continues right after it). Deliberately
/// excludes the straight apostrophe `'`, which in this dictionary's data
/// much more commonly marks an elided word form than a closing quote, so
/// stacking it here would generate mostly-noise combinations.
pub const CLOSING: &[&str] = &["\u{00BB}", ")", "\""];

/// Sentence-level marks that commonly follow a closing quote/bracket mark
/// with no space: comma, period, ano teleia, Greek question mark, colon.
pub const SENTENCE: &[&str] = &[",", ".", "\u{0387}", "\u{037E}", ":"];

/// All `{form}{closing}{sentence}` two-mark trailing combinations for one
/// form (e.g. "παλιόπαιδο»," "παλιόπαιδο")." ...). Kept as a separate
/// function from `variants_for` rather than folded in automatically: the
/// combination space (`CLOSING.len() * SENTENCE.len()` per form) grows
/// faster than the single-mark list, so callers decide explicitly whether
/// and how to scope it instead of getting it for free.
pub fn double_trailing_variants_for(form: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(CLOSING.len() * SENTENCE.len());
    for c in CLOSING {
        for s in SENTENCE {
            out.push(format!("{}{}{}", form, c, s));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_trailing_comma_and_period() {
        let v = variants_for("λέξη");
        assert!(v.contains(&"λέξη,".to_string()));
        assert!(v.contains(&"λέξη.".to_string()));
    }

    #[test]
    fn generates_trailing_ano_teleia() {
        let v = variants_for("λέξη");
        assert!(v.contains(&format!("λέξη{}", "\u{0387}")));
    }

    #[test]
    fn generates_leading_guillemet() {
        let v = variants_for("λέξη");
        assert!(v.contains(&"«λέξη".to_string()));
    }

    #[test]
    fn variant_count_matches_mark_lists() {
        let v = variants_for("λέξη");
        assert_eq!(v.len(), TRAILING.len() + LEADING.len());
    }

    #[test]
    fn generates_stacked_guillemet_comma() {
        let v = double_trailing_variants_for("παλιόπαιδο");
        assert!(v.contains(&"παλιόπαιδο»,".to_string()));
    }

    #[test]
    fn double_trailing_count_matches_mark_lists() {
        let v = double_trailing_variants_for("λέξη");
        assert_eq!(v.len(), CLOSING.len() * SENTENCE.len());
    }

    #[test]
    fn double_trailing_excludes_apostrophe_as_closing_mark() {
        let v = double_trailing_variants_for("λέξη");
        assert!(!v.iter().any(|s| s.starts_with("λέξη',") || s.starts_with("λέξη'.")));
    }
}
