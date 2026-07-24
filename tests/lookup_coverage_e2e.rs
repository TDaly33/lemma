// End-to-end validation of the lookup-coverage features (enclitic
// double-accentuation + punctuation variants) against the REAL generation
// pipeline (EntryProcessor -> HtmlGenerator -> kindling MOBI/StarDict),
// using a small synthetic kaikki-schema JSONL fixture instead of a live
// Wiktionary download. This exercises actual byte output (content_NN.html,
// a real .mobi, a real StarDict bundle) rather than just the in-process
// entry_variations() unit tests, without requiring network access.

use lemma::entry_processor::EntryProcessor;
use lemma::html_gen::{BuildParams, HtmlGenerator};
use lemma::mobi::MobiGenerator;
use lemma::stardict::StarDictGenerator;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

// Base fixture: no headword collides with any generated double-accent
// variant, so Feature 1's normal-case behavior can be tested cleanly.
const FIXTURE: &str = r#"{"word": "χαρτοφύλακας", "lang_code": "el", "pos": "noun", "senses": [{"glosses": ["briefcase"]}], "forms": [{"form": "χαρτοφύλακα", "tags": ["accusative", "singular"]}, {"form": "χαρτοφύλακες", "tags": ["nominative", "plural"]}, {"form": "χαρτοφυλάκων", "tags": ["genitive", "plural"]}]}
{"word": "άνθρωπος", "lang_code": "el", "pos": "noun", "senses": [{"glosses": ["human being"]}], "forms": [{"form": "ανθρώπου", "tags": ["genitive", "singular"]}, {"form": "άνθρωποι", "tags": ["nominative", "plural"]}, {"form": "ανθρώπους", "tags": ["accusative", "plural"]}]}
{"word": "λέξη", "lang_code": "el", "pos": "noun", "senses": [{"glosses": ["word"]}], "forms": [{"form": "λέξης", "tags": ["genitive", "singular"]}, {"form": "λέξεις", "tags": ["nominative", "plural"]}]}
{"word": "σπίτι", "lang_code": "el", "pos": "noun", "senses": [{"glosses": ["house"]}], "forms": [{"form": "σπιτιού", "tags": ["genitive", "singular"]}, {"form": "σπίτια", "tags": ["nominative", "plural"]}]}
{"word": "καλός", "lang_code": "el", "pos": "adj", "senses": [{"glosses": ["good"]}], "forms": [{"form": "καλή", "tags": ["feminine"]}, {"form": "καλό", "tags": ["neuter"]}]}
"#;

// Collision fixture: same as above, PLUS a fictitious real, distinct
// headword spelled exactly like χαρτοφύλακα's enclitic double-accent
// transform. Kept separate from FIXTURE so the "normal case works" tests
// and the "collision gets dropped" test don't contradict each other over
// the same string.
const COLLISION_FIXTURE: &str = r#"{"word": "χαρτοφύλακας", "lang_code": "el", "pos": "noun", "senses": [{"glosses": ["briefcase"]}], "forms": [{"form": "χαρτοφύλακα", "tags": ["accusative", "singular"]}]}
{"word": "χαρτοφύλακά", "lang_code": "el", "pos": "verb", "senses": [{"glosses": ["fictitious distinct headword, used only to test Feature 1 collision handling"]}], "forms": []}
"#;

// Punctuation-collision fixture: "καλά" is both its own headword (adverb)
// and an inflected form of the separate headword "καλός" - ordinary Greek
// syncretism, and exactly the real-data scenario that exposed Feature 2's
// missing cross-headword collision resolution. Both independently want the
// punctuation-attached candidate "καλά,".
const PUNCT_COLLISION_FIXTURE: &str = r#"{"word": "καλά", "lang_code": "el", "pos": "adv", "senses": [{"glosses": ["well"]}], "forms": []}
{"word": "καλός", "lang_code": "el", "pos": "adj", "senses": [{"glosses": ["good"]}], "forms": [{"form": "καλά", "tags": ["neuter", "plural"]}]}
"#;

struct Fixture {
    jsonl_path: PathBuf,
    output_dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.jsonl_path);
        let _ = fs::remove_dir_all(&self.output_dir);
    }
}

// cargo test runs test functions in parallel within one binary, and this
// file's tests each build a fixture dictionary (some more than once), so
// every invocation needs its own jsonl path and output directory - reusing
// one would race across threads.
static FIXTURE_ID: AtomicU32 = AtomicU32::new(0);

/// Runs the real pipeline (no network) against `jsonl` and returns the
/// fixture handle (whose Drop cleans up generated files) plus the
/// concatenated text of every generated content_NN.html file.
fn build_fixture_dictionary(jsonl: &str, punct_variants: usize) -> (Fixture, String) {
    let id = FIXTURE_ID.fetch_add(1, Ordering::SeqCst);
    // Distinctive limit-percent suffix (unique per call) keeps this test's
    // output directory from ever colliding with a real full build's
    // `lemma_greek_el/` or with another concurrently-running test.
    let limit_percent = 80.0 + id as f64;
    let jsonl_path = PathBuf::from(format!("test_fixture_{}.jsonl", id));
    {
        let mut f = fs::File::create(&jsonl_path).expect("write fixture jsonl");
        f.write_all(jsonl.as_bytes()).expect("write fixture jsonl");
    }

    let mut processor = EntryProcessor::new("el", None, jsonl_path.to_str().unwrap(), None);
    processor.process();
    assert!(!processor.entries.is_empty(), "fixture should parse into at least one entry");

    let params = BuildParams {
        source_lang: "el".to_string(),
        build_date: "20260101".to_string(),
        extraction_date: None,
        limit_percent: Some(limit_percent),
        max_inflections: None,
        punct_variants,
        front_matter: serde_json::json!({}),
    };

    let mut html_gen = HtmlGenerator::new(processor.entries, params, None);
    html_gen.create_output_files().expect("create_output_files should succeed");
    let output_dir = html_gen.output_dir.clone();

    let mut combined = String::new();
    let mut content_files: Vec<PathBuf> = fs::read_dir(&output_dir)
        .expect("output dir should exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("content_") && n.ends_with(".html"))
                .unwrap_or(false)
        })
        .collect();
    content_files.sort();
    assert!(!content_files.is_empty(), "expected at least one content_NN.html file");
    for f in content_files {
        combined.push_str(&fs::read_to_string(f).expect("read content file"));
    }

    (Fixture { jsonl_path, output_dir }, combined)
}

#[test]
fn spec_example_chartofylakas_gets_enclitic_double_accent_iform() {
    let (_fixture, html) = build_fixture_dictionary(FIXTURE, 0);
    assert!(
        html.contains("<idx:iform value=\"χαρτοφύλακά\""),
        "expected χαρτοφύλακά (enclitic double-accent of χαρτοφύλακα) as an iform"
    );
    // Regression guard: the unmodified single-accent form must still work.
    assert!(
        html.contains("<idx:iform value=\"χαρτοφύλακα\""),
        "expected the plain accusative form χαρτοφύλακα to still be indexed"
    );
}

#[test]
fn spec_example_anthropos_headword_gets_enclitic_double_accent_iform() {
    let (_fixture, html) = build_fixture_dictionary(FIXTURE, 0);
    assert!(
        html.contains("<idx:iform value=\"άνθρωπός\""),
        "expected άνθρωπός (enclitic double-accent of the headword άνθρωπος itself) as an iform"
    );
}

#[test]
fn paroxytone_lexi_does_not_get_a_double_accent_variant() {
    let (_fixture, html) = build_fixture_dictionary(FIXTURE, 0);
    // λέξη is paroxytone; it must never gain a second-accent iform.
    assert!(!html.contains("<idx:iform value=\"λέξή\""));
}

#[test]
fn double_accent_variant_never_shadows_a_real_distinct_headword() {
    let (_fixture, html) = build_fixture_dictionary(COLLISION_FIXTURE, 0);
    // "χαρτοφύλακά" is deliberately also a real, separate headword in the
    // fixture. It must appear as its own <idx:orth>, and must NOT also
    // appear as an <idx:iform> pointing at χαρτοφύλακας - that would send
    // Kindle lookups on the wrong meaning.
    assert!(
        html.contains("<idx:orth value=\"χαρτοφύλακά\">"),
        "the fictitious distinct headword should still get its own entry"
    );
    let iform_count = html.matches("<idx:iform value=\"χαρτοφύλακά\"").count();
    assert_eq!(
        iform_count, 0,
        "χαρτοφύλακά must not ALSO appear as an iform once it exists as a real headword"
    );
}

#[test]
fn punct_variant_never_appears_under_two_different_headwords() {
    // Regression guard for the collision bug found against the real
    // dictionary: before the fix, "καλά," would be independently generated
    // and emitted under BOTH <idx:entry id="hw_καλά"> and
    // <idx:entry id="hw_καλός">, an ambiguous/undefined Kindle lookup. It
    // must land under exactly one.
    let (_fixture, html) = build_fixture_dictionary(PUNCT_COLLISION_FIXTURE, 5);

    let kala_block = extract_entry_block(&html, "hw_καλά");
    let kalos_block = extract_entry_block(&html, "hw_καλός");

    let in_kala = kala_block.contains("<idx:iform value=\"καλά,\"");
    let in_kalos = kalos_block.contains("<idx:iform value=\"καλά,\"");

    assert!(
        in_kala ^ in_kalos,
        "\"καλά,\" must appear under exactly one entry: in_kala={}, in_kalos={}\nkala_block={}\nkalos_block={}",
        in_kala,
        in_kalos,
        kala_block,
        kalos_block
    );
}

fn extract_entry_block<'a>(html: &'a str, entry_id: &str) -> &'a str {
    let needle = format!("id=\"{}\"", entry_id);
    let start = html.find(&needle).expect("entry should exist");
    let end = html[start..].find("</idx:entry>").expect("entry should close") + start;
    &html[start..end]
}

#[test]
fn punctuation_variants_appear_when_enabled_and_not_when_disabled() {
    let (_fixture, html_on) = build_fixture_dictionary(FIXTURE, 5);
    assert!(html_on.contains("<idx:iform value=\"λέξη,\""));
    assert!(html_on.contains("<idx:iform value=\"λέξη.\""));
    assert!(html_on.contains("<idx:iform value=\"«λέξη\""));

    let (_fixture2, html_off) = build_fixture_dictionary(FIXTURE, 0);
    assert!(!html_off.contains("<idx:iform value=\"λέξη,\""));
}

#[test]
fn real_kindling_mobi_and_stardict_accept_the_expanded_iform_lists() {
    let (fixture, _html) = build_fixture_dictionary(FIXTURE, 10);

    let opf_filename = "lemma_greek_el.opf".to_string();
    let opf_path = fixture.output_dir.join(&opf_filename);
    assert!(opf_path.exists(), "opf should have been written");

    let mobi = MobiGenerator {
        output_dir: &fixture.output_dir,
        source_lang: "el",
        opf_filename: &opf_filename,
        is_full_build: false,
    };
    mobi.generate();
    let mobi_path = fixture.output_dir.join("lemma_greek_el.mobi");
    assert!(mobi_path.exists(), "kindling should have produced a .mobi file");
    assert!(fs::metadata(&mobi_path).unwrap().len() > 0);

    let stardict = StarDictGenerator {
        output_dir: &fixture.output_dir,
        source_lang: "el",
        opf_filename: &opf_filename,
        is_full_build: false,
    };
    stardict.generate();
    let has_stardict_dir = fs::read_dir(&fixture.output_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains("stardict"));
    assert!(has_stardict_dir, "kindling should have produced a stardict bundle directory");
}
