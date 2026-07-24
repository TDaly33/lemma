// Modern Greek monotonic stress-position detection and enclitic
// double-accentuation variant generation (Feature 1 of the lookup-coverage
// spec): proparoxytone headwords gain a second acute accent on the final
// syllable when an enclitic follows in running text, e.g. χαρτοφύλακα + του
// -> "χαρτοφύλακά του". The dictionary indexes only the citation form (one
// accent), so the enclitic-attached spelling (two accents) is a different
// Unicode string and fails exact Kindle lookup without this variant.
//
// NOTE on scope: monotonic Modern Greek orthography has no circumflex mark,
// so the Ancient Greek "properispomenon" accent class (circumflex on the
// penult) does not exist as a distinct category here - only the tonos
// position and syllable count matter. The enclitic-doubling rule applies
// exactly to proparoxytone words (stress on the third-from-last syllable),
// so that is the only class this module handles.

const ACUTE_MAP: &[(char, char)] = &[
    ('α', 'ά'), ('ε', 'έ'), ('η', 'ή'), ('ι', 'ί'), ('ο', 'ό'), ('υ', 'ύ'), ('ω', 'ώ'),
    ('Α', 'Ά'), ('Ε', 'Έ'), ('Η', 'Ή'), ('Ι', 'Ί'), ('Ο', 'Ό'), ('Υ', 'Ύ'), ('Ω', 'Ώ'),
    ('ϊ', 'ΐ'), ('ϋ', 'ΰ'),
];

fn is_vowel(c: char) -> bool {
    matches!(
        c,
        'α' | 'ε' | 'η' | 'ι' | 'ο' | 'υ' | 'ω'
            | 'ά' | 'έ' | 'ή' | 'ί' | 'ό' | 'ύ' | 'ώ'
            | 'ϊ' | 'ϋ' | 'ΐ' | 'ΰ'
            | 'Α' | 'Ε' | 'Η' | 'Ι' | 'Ο' | 'Υ' | 'Ω'
            | 'Ά' | 'Έ' | 'Ή' | 'Ί' | 'Ό' | 'Ύ' | 'Ώ'
            | 'Ϊ' | 'Ϋ'
    )
}

fn has_accent(c: char) -> bool {
    matches!(
        c,
        'ά' | 'έ' | 'ή' | 'ί' | 'ό' | 'ύ' | 'ώ' | 'ΐ' | 'ΰ'
            | 'Ά' | 'Έ' | 'Ή' | 'Ί' | 'Ό' | 'Ύ' | 'Ώ'
    )
}

fn has_diaeresis(c: char) -> bool {
    matches!(c, 'ϊ' | 'ϋ' | 'ΐ' | 'ΰ' | 'Ϊ' | 'Ϋ')
}

/// Lowercase "base" vowel identity (accent/diaeresis stripped), used only for
/// diphthong/synizesis classification - never for output.
fn base_vowel(c: char) -> Option<char> {
    Some(match c {
        'α' | 'ά' | 'Α' | 'Ά' => 'α',
        'ε' | 'έ' | 'Ε' | 'Έ' => 'ε',
        'η' | 'ή' | 'Η' | 'Ή' => 'η',
        'ι' | 'ί' | 'ϊ' | 'ΐ' | 'Ι' | 'Ί' | 'Ϊ' => 'ι',
        'ο' | 'ό' | 'Ο' | 'Ό' => 'ο',
        'υ' | 'ύ' | 'ϋ' | 'ΰ' | 'Υ' | 'Ύ' | 'Ϋ' => 'υ',
        'ω' | 'ώ' | 'Ω' | 'Ώ' => 'ω',
        _ => return None,
    })
}

fn add_acute(c: char) -> Option<char> {
    ACUTE_MAP.iter().find(|&&(base, _)| base == c).map(|&(_, acc)| acc)
}

/// Recognized vowel-digraph nuclei that count as a single syllable (spelled
/// with two vowel letters): αι, ει, οι, ου, υι. Merging is suppressed when
/// the first letter carries the tonos (that spells hiatus, not a digraph,
/// e.g. νερ-ά-ι-δα) or the second letter carries a diaeresis.
fn is_digraph_pair(first_base: char, second_base: char) -> bool {
    matches!(
        (first_base, second_base),
        ('α', 'ι') | ('ε', 'ι') | ('ο', 'ι') | ('ο', 'υ') | ('υ', 'ι')
    )
}

/// Splits `chars` into syllable-nucleus ranges (start, end) over the char
/// slice, applying digraph merging and ι-synizesis: an unstressed ι sitting
/// between a consonant and a following vowel glides into that vowel's
/// syllable rather than forming its own (e.g. the -δια in πόδια is one
/// syllable, not two - without this, πόδια would be miscounted as
/// proparoxytone).
fn find_nuclei(chars: &[char]) -> Vec<(usize, usize)> {
    let mut nuclei = Vec::new();
    let mut i = 0;
    let n = chars.len();
    while i < n {
        if !is_vowel(chars[i]) {
            i += 1;
            continue;
        }

        if i + 1 < n && is_vowel(chars[i + 1])
            && let (Some(fb), Some(sb)) = (base_vowel(chars[i]), base_vowel(chars[i + 1]))
            && is_digraph_pair(fb, sb) && !has_accent(chars[i]) && !has_diaeresis(chars[i + 1])
        {
            nuclei.push((i, i + 2));
            i += 2;
            continue;
        }

        if base_vowel(chars[i]) == Some('ι')
            && !has_accent(chars[i])
            && !has_diaeresis(chars[i])
            && i > 0
            && !is_vowel(chars[i - 1])
            && i + 1 < n
            && is_vowel(chars[i + 1])
        {
            // Synizesis: glide into the following syllable, no nucleus of its own.
            i += 1;
            continue;
        }

        nuclei.push((i, i + 1));
        i += 1;
    }
    nuclei
}

/// Distance of the stressed syllable from the end (1 = final/oxytone,
/// 2 = penult/paroxytone, 3 = antepenult/proparoxytone). `None` if the word
/// carries no tonos mark at all.
pub fn stress_position_from_end(word: &str) -> Option<usize> {
    let chars: Vec<char> = word.chars().collect();
    let nuclei = find_nuclei(&chars);
    let accented_idx = nuclei
        .iter()
        .position(|&(s, e)| chars[s..e].iter().any(|&c| has_accent(c)))?;
    Some(nuclei.len() - accented_idx)
}

/// True if `word` is stressed on the antepenultimate syllable (proparoxytone),
/// the class that gains a second acute on the final syllable before an
/// enclitic.
pub fn is_proparoxytone(word: &str) -> bool {
    stress_position_from_end(word) == Some(3)
}

/// If `word` is proparoxytone, returns the enclitic-attached spelling: the
/// same string with an additional acute added to the final syllable's vowel
/// (landing on the second letter when the final syllable is a digraph, per
/// normal Greek orthography), keeping the original antepenult accent
/// untouched, e.g. χαρτοφύλακα -> χαρτοφύλακά. Returns `None` for anything
/// else (oxytone, paroxytone, unaccented, or a final vowel with no
/// precomposed acute form).
pub fn enclitic_double_accent(word: &str) -> Option<String> {
    if !is_proparoxytone(word) {
        return None;
    }
    let mut chars: Vec<char> = word.chars().collect();
    let nuclei = find_nuclei(&chars);
    let (_, last_end) = *nuclei.last()?;
    let target = last_end - 1;
    chars[target] = add_acute(chars[target])?;
    Some(chars.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chartofylaka_is_proparoxytone_and_doubles() {
        assert!(is_proparoxytone("χαρτοφύλακα"));
        assert_eq!(enclitic_double_accent("χαρτοφύλακα").as_deref(), Some("χαρτοφύλακά"));
    }

    #[test]
    fn anthropos_is_proparoxytone_and_doubles() {
        assert!(is_proparoxytone("άνθρωπος"));
        assert_eq!(enclitic_double_accent("άνθρωπος").as_deref(), Some("άνθρωπός"));
    }

    #[test]
    fn paroxytone_word_is_not_eligible() {
        assert!(!is_proparoxytone("λέξη"));
        assert_eq!(enclitic_double_accent("λέξη"), None);
    }

    #[test]
    fn oxytone_word_is_not_eligible() {
        assert!(!is_proparoxytone("καλός"));
        assert_eq!(enclitic_double_accent("καλός"), None);
    }

    #[test]
    fn unaccented_word_is_not_eligible() {
        assert_eq!(stress_position_from_end("ανθρωπος"), None);
        assert_eq!(enclitic_double_accent("ανθρωπος"), None);
    }

    #[test]
    fn digraph_final_syllable_takes_accent_on_second_letter() {
        // "άνθρωποι" - proparoxytone plural, final syllable is the digraph
        // "οι"; the added accent must land on the second letter (ι), not the
        // first, matching normal Greek orthography for accented digraphs.
        assert!(is_proparoxytone("άνθρωποι"));
        assert_eq!(enclitic_double_accent("άνθρωποι").as_deref(), Some("άνθρωποί"));
    }

    #[test]
    fn synizesis_keeps_podia_paroxytone() {
        // "πόδια" (feet) is stressed πό-δια (2 syllables, paroxytone) because
        // the unstressed ι glides into the following vowel. Without
        // synizesis handling this would miscount as 3 syllables and wrongly
        // flag it proparoxytone.
        assert_eq!(stress_position_from_end("πόδια"), Some(2));
        assert!(!is_proparoxytone("πόδια"));
    }
}
