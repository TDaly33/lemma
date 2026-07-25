// Generates HTML files for the dictionary.

use crate::dilemma::DilemmaInflections;
use crate::entry_processor::{Entry, EntryMap, Example};
use crate::frequency::FrequencyRanker;
use crate::html_escape::escape_html;
use chrono::{Datelike, Local};
use rayon::prelude::*;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

pub const MAX_INFLECTIONS: usize = 255;
pub const MAX_POLYTONIC: usize = 255;

fn pos_map() -> &'static HashMap<&'static str, &'static str> {
    static M: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("noun", "ουσ.");
        m.insert("verb", "ρ.");
        m.insert("participle", "μτχ.");
        m.insert("adj", "επίθ.");
        m.insert("adjective", "επίθ.");
        m.insert("adv", "επίρρ.");
        m.insert("adverb", "επίρρ.");
        m.insert("num", "αριθμ.");
        m.insert("numeral", "αριθμ.");
        m.insert("name", "κύρ.όν.");
        m.insert("proper noun", "κύρ.όν.");
        m.insert("article", "άρθρ.");
        m
    })
}

fn strip_tags() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| {
        let mut s = HashSet::new();
        s.insert("masculine");
        s.insert("feminine");
        s.insert("neuter");
        s.insert("participle");
        s.insert("singular");
        s.insert("plural");
        s
    })
}

fn greek_word_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"([\u0370-\u03FF\u1F00-\u1FFF]+)").unwrap())
}

// Breathing maps: monotonic -> (smooth, rough)
fn breathing() -> &'static HashMap<char, (char, char)> {
    static M: OnceLock<HashMap<char, (char, char)>> = OnceLock::new();
    M.get_or_init(|| {
        let data: &[(char, (char, char))] = &[
            ('α', ('ἀ', 'ἁ')), ('ά', ('ἄ', 'ἅ')),
            ('ε', ('ἐ', 'ἑ')), ('έ', ('ἔ', 'ἕ')),
            ('η', ('ἠ', 'ἡ')), ('ή', ('ἤ', 'ἥ')),
            ('ι', ('ἰ', 'ἱ')), ('ί', ('ἴ', 'ἵ')),
            ('ο', ('ὀ', 'ὁ')), ('ό', ('ὄ', 'ὅ')),
            ('υ', ('ὐ', 'ὑ')), ('ύ', ('ὔ', 'ὕ')),
            ('ω', ('ὠ', 'ὡ')), ('ώ', ('ὤ', 'ὥ')),
            ('Α', ('Ἀ', 'Ἁ')), ('Ά', ('Ἄ', 'Ἅ')),
            ('Ε', ('Ἐ', 'Ἑ')), ('Έ', ('Ἔ', 'Ἕ')),
            ('Η', ('Ἠ', 'Ἡ')), ('Ή', ('Ἤ', 'Ἥ')),
            ('Ι', ('Ἰ', 'Ἱ')), ('Ί', ('Ἴ', 'Ἵ')),
            ('Ο', ('Ὀ', 'Ὁ')), ('Ό', ('Ὄ', 'Ὅ')),
            ('Υ', ('Ὑ', 'Ὑ')), ('Ύ', ('Ὕ', 'Ὕ')),
            ('Ω', ('Ὠ', 'Ὡ')), ('Ώ', ('Ὤ', 'Ὥ')),
        ];
        data.iter().cloned().collect()
    })
}

fn acute_to_grave() -> &'static HashMap<char, char> {
    static M: OnceLock<HashMap<char, char>> = OnceLock::new();
    M.get_or_init(|| {
        let data: &[(char, char)] = &[
            ('ά', 'ὰ'), ('έ', 'ὲ'), ('ή', 'ὴ'), ('ί', 'ὶ'), ('ό', 'ὸ'), ('ύ', 'ὺ'), ('ώ', 'ὼ'),
            ('Ά', 'Ὰ'), ('Έ', 'Ὲ'), ('Ή', 'Ὴ'), ('Ί', 'Ὶ'), ('Ό', 'Ὸ'), ('Ύ', 'Ὺ'), ('Ώ', 'Ὼ'),
        ];
        data.iter().cloned().collect()
    })
}

fn acute_to_circumflex() -> &'static HashMap<char, char> {
    static M: OnceLock<HashMap<char, char>> = OnceLock::new();
    M.get_or_init(|| {
        let data: &[(char, char)] = &[('ά', 'ᾶ'), ('ή', 'ῆ'), ('ί', 'ῖ'), ('ύ', 'ῦ'), ('ώ', 'ῶ')];
        data.iter().cloned().collect()
    })
}

// Accented-capital vowel -> bare-capital vowel, for dropping the tonos on a
// capitalized initial letter (Greek typesetting convention). Only the seven
// monotonic acute-accented capitals are relevant here - capitalize_first()
// (Rust's char::to_uppercase()) never produces anything else as the first
// character of a capitalized monotonic Greek word.
fn drop_tonos_from_capital(capitalized: &str) -> Option<String> {
    let mut chars = capitalized.chars();
    let first = chars.next()?;
    let bare = match first {
        'Ά' => 'Α',
        'Έ' => 'Ε',
        'Ή' => 'Η',
        'Ί' => 'Ι',
        'Ό' => 'Ο',
        'Ύ' => 'Υ',
        'Ώ' => 'Ω',
        _ => return None,
    };
    Some(format!("{}{}", bare, chars.as_str()))
}

const DIPHTHONG_FIRSTS: &str = "αεοΑΕΟ";
const DIPHTHONG_SECONDS: &str = "ιυίύΙΥΊΎ";

// --- Public API types ---

pub struct BuildParams {
    pub source_lang: String,
    /// Today's date as YYYYMMDD - displayed on the copyright page as the
    /// "Dictionary created" line. NOT used in any filename.
    pub build_date: String,
    pub extraction_date: Option<String>,
    pub limit_percent: Option<f64>,
    pub max_inflections: Option<usize>,
    /// Number of top-ranked inflected forms per headword (plus the headword
    /// itself) that get trailing/leading punctuation `<idx:iform>` variants.
    /// 0 disables Feature 2 entirely. See `entry_variations`.
    pub punct_variants: usize,
    pub front_matter: Value,
}

pub struct HtmlGenerator<'a> {
    pub entries: EntryMap,
    pub output_dir: PathBuf,
    pub params: BuildParams,
    dilemma: Option<&'a DilemmaInflections>,
    frequency: FrequencyRanker,
    use_ranked_forms: bool,
    iform_owner: HashMap<String, String>,
    // Per-headword enclitic double-accentuation variants (Feature 1),
    // pre-resolved for collisions against real headwords and against other
    // headwords' claims on the same string. Built by
    // `build_double_accent_variants` before rendering starts.
    double_accent_variants: HashMap<String, Vec<String>>,
    // Per-headword punctuation-attached variants (Feature 2), pre-resolved
    // for cross-headword collisions the same way as Feature 1. Built by
    // `build_punct_variants` before rendering starts.
    punct_variants_map: HashMap<String, Vec<String>>,
    // Per-headword explicit capitalized variants, pre-resolved for
    // cross-headword collisions the same way as Feature 1/2. Built by
    // `build_capitalized_variants` before rendering starts. Replaces the old
    // forms-array-gated capitalize_first() calls in entry_processor.rs -
    // this pass is unconditional on headword + every inflection, closing the
    // indeclinable-word capitalization gap (Όχι, Μα, Έτσι, ...) as a side
    // effect.
    capitalized_variants: HashMap<String, Vec<String>>,
    pub opf_filename: String,
    // Populated by create_content_html before any entry is rendered, so
    // linkify_definition can emit file-qualified cross-reference hrefs.
    // headword_buckets maps each headword to its content-file index (0..N);
    // large letters are split across several files, so this is a file index,
    // not a letter index.
    headword_buckets: HashMap<String, u8>,
    buckets_used: Vec<u8>,
    // Per-file display label (indexed by file index) and one TOC nav point per
    // letter (file index of the letter's first file + its label), both built
    // alongside headword_buckets.
    file_labels: Vec<String>,
    nav_points: Vec<(u8, String)>,
}

impl<'a> HtmlGenerator<'a> {
    pub fn new(entries: EntryMap, params: BuildParams, dilemma: Option<&'a DilemmaInflections>) -> Self {
        let use_ranked_forms = dilemma.as_ref().map(|d| d.has_ranked_forms()).unwrap_or(false);
        if use_ranked_forms {
            println!("Using pre-ranked forms from dilemma for inflection ordering");
        }
        let freq = FrequencyRanker::new();

        // Build directory name has no date and no version - lemma's stable
        // naming convention. The limit-percent suffix stays because it
        // identifies a test build vs. the real one.
        let mut output_dir = format!("lemma_greek_{}", params.source_lang);
        if let Some(p) = params.limit_percent {
            output_dir = format!("{}_{}pct", output_dir, p);
        }

        Self {
            entries,
            output_dir: PathBuf::from(output_dir),
            params,
            dilemma,
            frequency: freq,
            use_ranked_forms,
            iform_owner: HashMap::new(),
            double_accent_variants: HashMap::new(),
            punct_variants_map: HashMap::new(),
            capitalized_variants: HashMap::new(),
            opf_filename: String::new(),
            headword_buckets: HashMap::new(),
            buckets_used: Vec::new(),
            file_labels: Vec::new(),
            nav_points: Vec::new(),
        }
    }

    pub fn create_output_files(&mut self) -> std::io::Result<()> {
        if self.output_dir.is_dir() {
            println!("Removing existing directory: {}", self.output_dir.display());
            fs::remove_dir_all(&self.output_dir)?;
        }
        fs::create_dir_all(&self.output_dir)?;

        self.create_content_html()?;
        self.create_cover()?;
        self.create_copyright_html()?;
        self.create_usage_html()?;
        self.create_opf_file()?;
        self.create_toc_ncx()?;
        Ok(())
    }

    fn create_content_html(&mut self) -> std::io::Result<()> {
        println!("Creating per-letter content files...");

        self.merge_form_of_into_parents();
        self.build_iform_owners();
        self.build_double_accent_variants();
        self.build_punct_variants();
        self.build_capitalized_variants();

        // Sort keys by normalized form
        let mut sorted_keys: Vec<String> = self.entries.keys().cloned().collect();
        sorted_keys.sort_by_cached_key(|k| normalize_for_sorting(k));

        // Assign every headword to a content file before rendering, so
        // linkify_definition can emit cross-reference hrefs that point at the
        // correct file. Each Greek letter (plus the non-Greek "misc" bucket)
        // starts a fresh file, and any letter holding more than
        // MAX_ENTRIES_PER_FILE entries is spread across several consecutive
        // files. This keeps every file well under KDP's server-side converter
        // limit (KPG §15.5) and satisfies §15.2's "each alphabet letter
        // section should begin on a new page" as a side effect. Α is why this
        // matters: the privative α-/αν- plus the ανα-/απο-/αντι- prefixes give
        // it ~4.5x the headwords of any other letter, so as a single file it
        // was ~19 MB and stalled the converter.
        //
        // Tunable: lower MAX_ENTRIES_PER_FILE if the converter still struggles.
        // It only changes how content is chunked, never the per-entry markup,
        // so the MOBI/kindling path is unaffected.
        const MAX_ENTRIES_PER_FILE: usize = 2500;

        // Group headwords by letter bucket, preserving sorted order within each.
        let mut by_letter: HashMap<u8, Vec<&String>> = HashMap::new();
        for k in &sorted_keys {
            by_letter.entry(bucket_for_headword(k)).or_default().push(k);
        }
        let mut letters: Vec<u8> = by_letter.keys().copied().collect();
        letters.sort();

        // Walk letters in alphabetical order, emitting one or more files per
        // letter. Record the headword -> file-index map, a label per file (for
        // the <title>), and one TOC nav point per letter (its first file) so
        // the table of contents stays one entry per letter.
        let mut headword_file: HashMap<String, u8> = HashMap::new();
        let mut file_labels: Vec<String> = Vec::new();
        let mut nav_points: Vec<(u8, String)> = Vec::new();
        let mut file_idx: u8 = 0;
        for letter in &letters {
            let words = &by_letter[letter];
            let label = bucket_label(*letter);
            let nfiles = ((words.len() + MAX_ENTRIES_PER_FILE - 1) / MAX_ENTRIES_PER_FILE).max(1);
            let per = (words.len() + nfiles - 1) / nfiles;
            nav_points.push((file_idx, label.to_string()));
            for (part, chunk) in words.chunks(per).enumerate() {
                for w in chunk {
                    headword_file.insert((*w).clone(), file_idx);
                }
                file_labels.push(if nfiles > 1 {
                    format!("{} ({})", label, part + 1)
                } else {
                    label.to_string()
                });
                file_idx = file_idx
                    .checked_add(1)
                    .expect("content file count exceeds u8 range");
            }
        }

        self.headword_buckets = headword_file;
        self.file_labels = file_labels;
        self.nav_points = nav_points;

        // Render each entry in parallel, tagged with its content-file index.
        let rendered: Vec<(u8, Vec<u8>)> = sorted_keys
            .par_iter()
            .map(|word| {
                let entries = self.entries.get(word).map(|v| v.as_slice()).unwrap_or(&[]);
                let mut buf: Vec<u8> = Vec::with_capacity(512);
                let _ = self.write_entry(&mut buf, word, entries);
                let file = self.headword_buckets.get(word).copied().unwrap_or(0);
                (file, buf)
            })
            .collect();

        // Group by file index while preserving the sort order within each file.
        let mut per_file: HashMap<u8, Vec<Vec<u8>>> = HashMap::new();
        for (file, buf) in rendered {
            per_file.entry(file).or_default().push(buf);
        }

        let mut files_used: Vec<u8> = per_file.keys().copied().collect();
        files_used.sort();
        self.buckets_used = files_used.clone();

        let mut total_entries = 0usize;
        for file in files_used {
            let bufs = &per_file[&file];
            let path = self.output_dir.join(bucket_filename(file));
            let mut out = BufWriter::new(File::create(&path)?);
            out.write_all(html_header(&self.file_labels[file as usize]).as_bytes())?;
            for buf in bufs {
                out.write_all(buf)?;
            }
            out.write_all(html_footer().as_bytes())?;
            out.flush()?;
            total_entries += bufs.len();
            println!(
                "  Wrote {} ({}: {} entries)",
                bucket_filename(file),
                self.file_labels[file as usize],
                bufs.len()
            );
        }
        println!(
            "  Created {} content file(s) with {} entries total",
            self.buckets_used.len(),
            total_entries
        );

        let mut total_bytes: u64 = 0;
        for &file in &self.buckets_used {
            if let Ok(meta) = fs::metadata(self.output_dir.join(bucket_filename(file))) {
                total_bytes += meta.len();
            }
        }
        println!(
            "  Total content size: {:.2} MB",
            total_bytes as f64 / 1024.0 / 1024.0
        );
        Ok(())
    }

    fn merge_form_of_into_parents(&mut self) {
        let mut to_remove: Vec<String> = Vec::new();
        let mut merged_count = 0;

        // Collect candidates
        let candidates: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, es)| es.iter().all(|e| !e.form_of_targets.is_empty()))
            .map(|(k, _)| k.clone())
            .collect();

        for word in candidates {
            let targets: Vec<String> = {
                let entries = self.entries.get(&word).unwrap();
                let mut seen = HashSet::new();
                let mut ts = Vec::new();
                for e in entries {
                    for t in &e.form_of_targets {
                        if !seen.contains(t) && self.entries.contains_key(t) {
                            ts.push(t.clone());
                            seen.insert(t.clone());
                        }
                    }
                }
                ts
            };

            if targets.is_empty() { continue; }

            let best = if targets.len() == 1 {
                targets[0].clone()
            } else {
                // Match Python's `max(targets, key=...)` tiebreak: first element wins.
                let freq = &self.frequency;
                let mut best = targets[0].clone();
                let mut best_f = freq.frequency(&best);
                for t in targets.iter().skip(1) {
                    let f = freq.frequency(t);
                    if f > best_f {
                        best = t.clone();
                        best_f = f;
                    }
                }
                best
            };

            if let Some(parent_entries) = self.entries.get_mut(&best) {
                for pe in parent_entries.iter_mut() {
                    if !pe.inflections.contains(&word) {
                        pe.inflections.push(word.clone());
                    }
                }
            }
            to_remove.push(word);
            merged_count += 1;
        }

        let set: HashSet<String> = to_remove.into_iter().collect();
        self.entries.remove_many(&set);
        println!("  Merged {} form-of entries into parent headwords", merged_count);
    }

    fn build_iform_owners(&mut self) {
        let mut all_claims: HashMap<String, Vec<String>> = HashMap::new();

        for (word, entries) in self.entries.iter() {
            for e in entries {
                for inf in &e.inflections {
                    let claims = all_claims.entry(inf.clone()).or_default();
                    if !claims.contains(word) {
                        claims.push(word.clone());
                    }
                }
            }
            if self.use_ranked_forms {
                if let Some(dilemma) = self.dilemma {
                    if let Some(ranked) = dilemma.get_ranked_forms(word) {
                        for inf in ranked {
                            let claims = all_claims.entry(inf.clone()).or_default();
                            if !claims.contains(word) {
                                claims.push(word.clone());
                            }
                        }
                    }
                }
            }
        }

        let equiv_canonical: HashMap<String, String> = self
            .dilemma
            .map(|d| d.equivalences.clone())
            .unwrap_or_default();

        let mut iform_owner: HashMap<String, String> = HashMap::new();
        let mut contested = 0;
        for (iform, headwords) in &all_claims {
            if headwords.len() <= 1 { continue; }

            // First preference: if one contestant is a variant of another
            // per dilemma's equivalences map, the canonical wins.
            let mut winner: Option<String> = None;
            for hw in headwords {
                if let Some(canonical) = equiv_canonical.get(hw) {
                    if headwords.contains(canonical) {
                        winner = Some(canonical.clone());
                        break;
                    }
                }
            }

            // Otherwise pick the highest-frequency headword. Alphabetical
            // stable tiebreak keeps output deterministic when frequencies
            // match (including the all-zero case).
            if winner.is_none() {
                let mut best: Option<(&String, i64)> = None;
                for hw in headwords {
                    let f = self.frequency.frequency(hw);
                    match &best {
                        None => best = Some((hw, f)),
                        Some((bw, bf)) => {
                            if f > *bf || (f == *bf && hw < *bw) {
                                best = Some((hw, f));
                            }
                        }
                    }
                }
                winner = best.map(|(w, _)| w.clone());
            }

            let Some(winner) = winner else { continue; };
            contested += 1;
            for hw in headwords {
                if hw != &winner {
                    iform_owner.insert(iform.clone(), winner.clone());
                }
            }
        }
        println!("  Deduplicated {} contested iforms (canonical form + highest-frequency tiebreak)", contested);
        self.iform_owner = iform_owner;
    }

    /// Precompute enclitic double-accentuation variants (Feature 1): for each
    /// headword and each of its generated inflected forms, if the form is
    /// proparoxytone (see `crate::accent`), add the enclitic-attached
    /// spelling (extra acute on the final syllable) as an index-only variant
    /// of that same headword.
    ///
    /// Runs once, single-threaded-safe collision resolution, before the
    /// parallel render pass so every headword sees the same final answer for
    /// contested strings:
    ///   - a candidate identical to an existing, distinct headword is
    ///     dropped entirely (it already has its own real entry) and logged;
    ///   - a candidate independently produced by more than one headword is
    ///     awarded to the highest-frequency headword (alphabetical tiebreak),
    ///     mirroring `build_iform_owners`'s existing contested-iform policy.
    fn build_double_accent_variants(&mut self) {
        // Each headword's own generated forms: itself plus every single-word
        // inflection from its entries (the same "generated inflected forms"
        // the rest of the pipeline works from, independent of the display
        // cap applied later in entry_variations).
        let per_word_forms: Vec<(String, Vec<String>)> = self
            .entries
            .iter()
            .map(|(word, entries)| {
                let mut forms: Vec<String> = vec![word.clone()];
                let mut seen: HashSet<String> = HashSet::new();
                seen.insert(word.clone());
                for e in entries {
                    for inf in &e.inflections {
                        if !inf.contains(' ') && seen.insert(inf.clone()) {
                            forms.push(inf.clone());
                        }
                    }
                }
                (word.clone(), forms)
            })
            .collect();

        let per_word_candidates: Vec<(String, Vec<String>)> = per_word_forms
            .par_iter()
            .map(|(word, forms)| {
                let mut seen: HashSet<String> = forms.iter().cloned().collect();
                let mut cands = Vec::new();
                for f in forms {
                    if let Some(v) = crate::accent::enclitic_double_accent(f)
                        && seen.insert(v.clone())
                    {
                        cands.push(v);
                    }
                }
                (word.clone(), cands)
            })
            .collect();

        let mut claims: HashMap<String, Vec<String>> = HashMap::new();
        for (word, cands) in &per_word_candidates {
            for c in cands {
                claims.entry(c.clone()).or_default().push(word.clone());
            }
        }

        let mut winner_for: HashMap<String, String> = HashMap::new();
        let mut headword_collisions: Vec<String> = Vec::new();
        let mut contested_resolved = 0usize;

        for (cand, claimants) in &claims {
            if self.entries.contains_key(cand) {
                headword_collisions.push(cand.clone());
                continue;
            }
            if claimants.len() == 1 {
                winner_for.insert(cand.clone(), claimants[0].clone());
                continue;
            }
            contested_resolved += 1;
            let mut best = claimants[0].clone();
            let mut best_f = self.frequency.frequency(&best);
            for w in claimants.iter().skip(1) {
                let f = self.frequency.frequency(w);
                if f > best_f || (f == best_f && *w < best) {
                    best = w.clone();
                    best_f = f;
                }
            }
            winner_for.insert(cand.clone(), best);
        }

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        let mut total_added = 0usize;
        for (word, cands) in per_word_candidates {
            for c in cands {
                if winner_for.get(&c).map(|w| w == &word).unwrap_or(false) {
                    result.entry(word.clone()).or_default().push(c);
                    total_added += 1;
                }
            }
        }

        if !headword_collisions.is_empty() {
            let mut sample = headword_collisions.clone();
            sample.sort();
            let shown: Vec<&str> = sample.iter().take(10).map(|s| s.as_str()).collect();
            println!(
                "  Enclitic double-accent: skipped {} variant(s) colliding with an existing distinct headword (manual review): {}{}",
                headword_collisions.len(),
                shown.join(", "),
                if headword_collisions.len() > 10 { ", ..." } else { "" }
            );
        }
        println!(
            "  Enclitic double-accent: added {} variant(s) across {} headword(s) ({} contested resolved by frequency)",
            total_added,
            result.len(),
            contested_resolved
        );

        self.double_accent_variants = result;
    }

    /// Precompute punctuation-attached variants (Feature 2): for each
    /// headword, the headword itself plus its top `punct_variants` ranked
    /// inflections each get trailing/leading punctuation forms. Resolved for
    /// cross-headword collisions dictionary-wide, before rendering starts -
    /// the same pattern as `build_double_accent_variants`, and for the same
    /// reason: two different headwords can independently produce the exact
    /// same punctuated string (e.g. both have a top-ranked form "λέξη"), and
    /// without this pass that collision was previously resolved arbitrarily
    /// by whatever kindling's INDX builder happened to keep last, with no
    /// tiebreak and no visibility into how often it occurred.
    fn build_punct_variants(&mut self) {
        let punct_n = self.params.punct_variants;
        if punct_n == 0 {
            self.punct_variants_map = HashMap::new();
            return;
        }

        // headword + its top-N ranked inflections (pre-polytonic, matching
        // Feature 2's original scoping) for every headword.
        let per_word_sources: Vec<(String, Vec<String>)> = self
            .entries
            .iter()
            .map(|(word, entries)| {
                let ranked = self.ranked_variations(word, entries);
                let mut source = Vec::with_capacity(punct_n + 1);
                source.push(word.clone());
                source.extend(ranked.into_iter().take(punct_n));
                (word.clone(), source)
            })
            .collect();

        let per_word_candidates: Vec<(String, Vec<String>)> = per_word_sources
            .par_iter()
            .map(|(word, source)| {
                let mut seen: HashSet<String> = HashSet::new();
                let mut cands = Vec::new();
                for form in source {
                    for v in crate::punctuation::variants_for(form) {
                        if seen.insert(v.clone()) {
                            cands.push(v);
                        }
                    }
                    // Two-mark trailing combinations (closing quote/bracket
                    // immediately followed by a sentence-level mark, e.g.
                    // "παλιόπαιδο»,") - same scope as the single-mark
                    // variants above, no separate cap.
                    for v in crate::punctuation::double_trailing_variants_for(form) {
                        if seen.insert(v.clone()) {
                            cands.push(v);
                        }
                    }
                }
                (word.clone(), cands)
            })
            .collect();

        let mut claims: HashMap<String, Vec<String>> = HashMap::new();
        for (word, cands) in &per_word_candidates {
            for c in cands {
                claims.entry(c.clone()).or_default().push(word.clone());
            }
        }

        let mut winner_for: HashMap<String, String> = HashMap::new();
        let mut headword_collisions: Vec<String> = Vec::new();
        let mut contested_resolved = 0usize;

        for (cand, claimants) in &claims {
            if self.entries.contains_key(cand) {
                headword_collisions.push(cand.clone());
                continue;
            }
            if claimants.len() == 1 {
                winner_for.insert(cand.clone(), claimants[0].clone());
                continue;
            }
            contested_resolved += 1;
            let mut best = claimants[0].clone();
            let mut best_f = self.frequency.frequency(&best);
            for w in claimants.iter().skip(1) {
                let f = self.frequency.frequency(w);
                if f > best_f || (f == best_f && *w < best) {
                    best = w.clone();
                    best_f = f;
                }
            }
            winner_for.insert(cand.clone(), best);
        }

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        let mut total_added = 0usize;
        for (word, cands) in per_word_candidates {
            for c in cands {
                if winner_for.get(&c).map(|w| w == &word).unwrap_or(false) {
                    result.entry(word.clone()).or_default().push(c);
                    total_added += 1;
                }
            }
        }

        if !headword_collisions.is_empty() {
            let mut sample = headword_collisions.clone();
            sample.sort();
            let shown: Vec<&str> = sample.iter().take(10).map(|s| s.as_str()).collect();
            println!(
                "  Punctuation variants: skipped {} variant(s) colliding with an existing distinct headword (manual review): {}{}",
                headword_collisions.len(),
                shown.join(", "),
                if headword_collisions.len() > 10 { ", ..." } else { "" }
            );
        }
        println!(
            "  Punctuation variants: added {} variant(s) across {} headword(s) ({} contested resolved by frequency, top {} ranked forms per headword)",
            total_added,
            result.len(),
            contested_resolved,
            punct_n
        );

        self.punct_variants_map = result;
    }

    /// Explicit capitalized-form variants: for every headword and every one
    /// of its generated inflections that doesn't already start with an
    /// uppercase letter, index up to two capitalized forms.
    ///
    /// 1. Accent-preserving capital (`capitalize_first`, Rust's
    ///    `char::to_uppercase()`): έδειχναν -> Έδειχναν.
    /// 2. Accent-dropped-on-first-letter capital, only when the
    ///    accent-preserving capital's first letter actually carries a tonos
    ///    (Ά/Έ/Ή/Ί/Ό/Ύ/Ώ -> Α/Ε/Η/Ι/Ο/Υ/Ω), matching the Greek typesetting
    ///    convention of dropping the accent on a capitalized initial letter:
    ///    έδειχναν -> Εδειχναν.
    ///
    /// Replaces the old capitalize_first()/lower_first() calls that used to
    /// live inside entry_processor.rs's loop over the raw Wiktionary `forms`
    /// array (gated on that array being non-empty). Being unconditional on
    /// headword + every inflection instead closes the indeclinable-word gap
    /// (Όχι, Μα, Έτσι, ...) as a side effect - those words have an empty
    /// `forms` array, so the old logic never produced a capitalized variant
    /// for them at all.
    ///
    /// Dictionary-wide collision resolution mirrors Feature 1/Feature 2
    /// exactly: a candidate colliding with a real distinct headword is
    /// dropped and logged; a candidate independently produced by two
    /// headwords is resolved by frequency (alphabetical tiebreak).
    fn build_capitalized_variants(&mut self) {
        let per_word_forms: Vec<(String, Vec<String>)> = self
            .entries
            .iter()
            .map(|(word, entries)| {
                let mut forms: Vec<String> = vec![word.clone()];
                let mut seen: HashSet<String> = HashSet::new();
                seen.insert(word.clone());
                for e in entries {
                    for inf in &e.inflections {
                        if !inf.contains(' ') && seen.insert(inf.clone()) {
                            forms.push(inf.clone());
                        }
                    }
                }
                (word.clone(), forms)
            })
            .collect();

        let per_word_candidates: Vec<(String, Vec<String>)> = per_word_forms
            .par_iter()
            .map(|(word, forms)| {
                let mut seen: HashSet<String> = forms.iter().cloned().collect();
                let mut cands = Vec::new();
                for f in forms {
                    let accent_preserving = crate::entry_processor::capitalize_first(f);
                    if accent_preserving == *f {
                        // Already starts uppercase (or first char isn't cased).
                        continue;
                    }
                    if seen.insert(accent_preserving.clone()) {
                        cands.push(accent_preserving.clone());
                    }
                    if let Some(accent_dropped) = drop_tonos_from_capital(&accent_preserving)
                        && seen.insert(accent_dropped.clone())
                    {
                        cands.push(accent_dropped);
                    }
                }
                (word.clone(), cands)
            })
            .collect();

        let mut claims: HashMap<String, Vec<String>> = HashMap::new();
        for (word, cands) in &per_word_candidates {
            for c in cands {
                claims.entry(c.clone()).or_default().push(word.clone());
            }
        }

        let mut winner_for: HashMap<String, String> = HashMap::new();
        let mut headword_collisions: Vec<String> = Vec::new();
        let mut contested_resolved = 0usize;

        for (cand, claimants) in &claims {
            if self.entries.contains_key(cand) {
                headword_collisions.push(cand.clone());
                continue;
            }
            if claimants.len() == 1 {
                winner_for.insert(cand.clone(), claimants[0].clone());
                continue;
            }
            contested_resolved += 1;
            let mut best = claimants[0].clone();
            let mut best_f = self.frequency.frequency(&best);
            for w in claimants.iter().skip(1) {
                let f = self.frequency.frequency(w);
                if f > best_f || (f == best_f && *w < best) {
                    best = w.clone();
                    best_f = f;
                }
            }
            winner_for.insert(cand.clone(), best);
        }

        let mut result: HashMap<String, Vec<String>> = HashMap::new();
        let mut total_added = 0usize;
        for (word, cands) in per_word_candidates {
            for c in cands {
                if winner_for.get(&c).map(|w| w == &word).unwrap_or(false) {
                    result.entry(word.clone()).or_default().push(c);
                    total_added += 1;
                }
            }
        }

        if !headword_collisions.is_empty() {
            let mut sample = headword_collisions.clone();
            sample.sort();
            let shown: Vec<&str> = sample.iter().take(10).map(|s| s.as_str()).collect();
            println!(
                "  Capitalized variants: skipped {} variant(s) colliding with an existing distinct headword (manual review): {}{}",
                headword_collisions.len(),
                shown.join(", "),
                if headword_collisions.len() > 10 { ", ..." } else { "" }
            );
        }
        println!(
            "  Capitalized variants: added {} variant(s) across {} headword(s) ({} contested resolved by frequency)",
            total_added,
            result.len(),
            contested_resolved
        );

        self.capitalized_variants = result;
    }

    /// Ranked single-word inflections for a headword, capped at
    /// `max_inflections` and de-duplicated against iforms owned by a
    /// higher-frequency headword. Shared by `entry_variations` (which
    /// extends this with polytonic/Feature-1/Feature-2 additions) and the
    /// upfront Feature 2 collision-resolution pass, which needs the same
    /// ranked/capped list to determine each headword's punctuation scope
    /// before any entry is rendered.
    fn ranked_variations(&self, word: &str, entries: &[Entry]) -> Vec<String> {
        let max_inflections = self.params.max_inflections.unwrap_or(MAX_INFLECTIONS);

        // Combine all inflections from all entries
        let mut all_inflections: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for e in entries {
            for inf in &e.inflections {
                if seen.insert(inf.clone()) {
                    all_inflections.push(inf.clone());
                }
            }
        }

        let single_word: Vec<String> = all_inflections.into_iter().filter(|i| !i.contains(' ')).collect();

        let is_proper_noun = entries.iter().any(|e| {
            let lower = e.pos.to_lowercase();
            lower.contains("proper") || lower.contains("name")
        });

        let mut all_variations: Vec<String> = if is_proper_noun {
            single_word.into_iter().take(max_inflections).collect()
        } else if self.use_ranked_forms {
            self.select_ranked_inflections(word, &single_word, max_inflections)
        } else {
            let ranked = self.rank_inflections(&single_word);
            ranked.into_iter().take(max_inflections).collect()
        };

        // Remove iforms owned by a higher-frequency headword
        all_variations.retain(|inf| {
            self.iform_owner.get(inf).map(|w| w.as_str() == word).unwrap_or(true)
        });

        all_variations
    }

    /// Compute the full ordered set of searchable inflected forms for a
    /// headword: ranked single-word inflections (capped at max_inflections),
    /// de-duplicated against iforms owned by a higher-frequency headword, then
    /// extended with polytonic variants. This is exactly the set emitted as
    /// `<idx:iform>` by the idx path and as `<match>` entries by the EPUB3 SKM,
    /// keeping the two outputs in agreement.
    fn entry_variations(&self, word: &str, entries: &[Entry]) -> Vec<String> {
        let mut all_variations = self.ranked_variations(word, entries);

        // Polytonic expansion (always enabled in the unified edition).
        {
            let mut all_forms: Vec<String> = Vec::with_capacity(all_variations.len() + 1);
            all_forms.push(word.to_string());
            all_forms.extend(all_variations.iter().cloned());
            let mut polytonic_forms: Vec<String> = Vec::new();
            let mut seen_poly: HashSet<String> = all_forms.iter().cloned().collect();

            let dilemma_has_poly = self.dilemma.map(|d| d.has_polytonic_ranked()).unwrap_or(false);
            if dilemma_has_poly {
                let dilemma = self.dilemma.unwrap();
                for form in &all_forms {
                    for pv in dilemma.get_polytonic_variants(form) {
                        if seen_poly.insert(pv.clone()) {
                            polytonic_forms.push(pv.clone());
                        }
                    }
                }
            } else {
                for form in &all_forms {
                    for pv in polytonic_variants(form) {
                        if seen_poly.insert(pv.clone()) {
                            polytonic_forms.push(pv);
                        }
                    }
                }
            }
            polytonic_forms.truncate(MAX_POLYTONIC);
            all_variations.extend(polytonic_forms);
        }

        // Feature 1: enclitic double-accentuation variants precomputed by
        // `build_double_accent_variants`, already collision-resolved.
        if let Some(extra) = self.double_accent_variants.get(word) {
            let mut seen: HashSet<String> = all_variations.iter().cloned().collect();
            for v in extra {
                if seen.insert(v.clone()) {
                    all_variations.push(v.clone());
                }
            }
        }

        // Feature 2: trailing/leading punctuation variants precomputed by
        // `build_punct_variants`, already collision-resolved.
        if let Some(extra) = self.punct_variants_map.get(word) {
            let mut seen: HashSet<String> = all_variations.iter().cloned().collect();
            for v in extra {
                if seen.insert(v.clone()) {
                    all_variations.push(v.clone());
                }
            }
        }

        // Explicit capitalized variants precomputed by
        // `build_capitalized_variants`, already collision-resolved.
        if let Some(extra) = self.capitalized_variants.get(word) {
            let mut seen: HashSet<String> = all_variations.iter().cloned().collect();
            for v in extra {
                if seen.insert(v.clone()) {
                    all_variations.push(v.clone());
                }
            }
        }

        all_variations
    }

    fn write_entry<W: Write>(&self, out: &mut W, word: &str, entries: &[Entry]) -> std::io::Result<()> {
        let all_variations = self.entry_variations(word, entries);

        let escaped_word = escape_html(word);
        let anchor = sanitize_anchor_id(&escaped_word);
        write!(out,
            "<idx:entry name=\"default\" scriptable=\"yes\" spell=\"yes\" id=\"hw_{}\">\n  <idx:short>\n    <idx:orth value=\"{}\"><b>{}</b>\n",
            anchor, escaped_word, escaped_word
        )?;

        if !all_variations.is_empty() {
            out.write_all(b"      <idx:infl>\n")?;
            for variation in &all_variations {
                write!(out, "        <idx:iform value=\"{}\" exact=\"yes\" />\n", escape_html(variation))?;
            }
            out.write_all(b"      </idx:infl>\n")?;
        }

        out.write_all(b"    </idx:orth>\n")?;
        out.write_all(b"  </idx:short>\n")?;

        self.write_entry_body(out, word, entries, "html")?;

        out.write_all(b"</idx:entry>\n<hr/>\n")?;
        Ok(())
    }

    /// Render the body of a single entry (POS groups, definitions, examples,
    /// etymology) shared by the idx path and the EPUB3 path. `link_ext` selects
    /// the file extension used for cross-reference hrefs ("html" for idx/MOBI,
    /// "xhtml" for EPUB3). Calling with "html" reproduces the historical idx
    /// body byte-for-byte.
    fn write_entry_body<W: Write>(&self, out: &mut W, word: &str, entries: &[Entry], link_ext: &str) -> std::io::Result<()> {
        if self.params.source_lang == "el" {
            // Group by POS preserving insertion order
            let mut pos_order: Vec<String> = Vec::new();
            let mut pos_groups: HashMap<String, Vec<&Entry>> = HashMap::new();
            for e in entries {
                let key = e.pos.clone();
                if !pos_groups.contains_key(&key) {
                    pos_order.push(key.clone());
                }
                pos_groups.entry(key).or_default().push(e);
            }

            let pos_count = pos_order.len();
            for (idx, pos) in pos_order.iter().enumerate() {
                let pos_entries = &pos_groups[pos];

                // Combine definitions and examples
                let mut all_definitions: Vec<String> = Vec::new();
                let mut all_examples: Vec<Option<Example>> = Vec::new();
                let mut def_seen: HashSet<String> = HashSet::new();
                for e in pos_entries {
                    for (i, d) in e.definitions.iter().enumerate() {
                        if def_seen.insert(d.clone()) {
                            all_definitions.push(d.clone());
                            let ex = e.examples.get(i).cloned().unwrap_or(None);
                            all_examples.push(ex);
                        }
                    }
                }

                let mut effective_pos = pos.clone();
                if pos == "verb" && !all_definitions.is_empty() {
                    if all_definitions.iter().all(|d| def_has_tag(d, "participle")) {
                        effective_pos = "participle".to_string();
                    }
                }

                let mut pos_display = self.format_pos(&effective_pos);
                if let Some(head_info) = self.get_head_info_for_pos(pos_entries.as_slice(), word) {
                    pos_display = format!("{}, {}", pos_display, head_info);
                }
                write!(out, "  <div><i>{}</i></div>\n", escape_html(&pos_display))?;

                let to_print = all_definitions.iter().take(5).collect::<Vec<_>>();
                for (def_idx, definition) in to_print.iter().enumerate() {
                    let clean = strip_def_qualifiers(definition);
                    if all_definitions.len() > 1 {
                        write!(out, "  <div class='def'>{}. {}</div>\n", def_idx + 1, self.linkify_definition_ext(&clean, link_ext))?;
                    } else {
                        write!(out, "  <div class='def'>{}</div>\n", self.linkify_definition_ext(&clean, link_ext))?;
                    }
                    if let Some(ex) = all_examples.get(def_idx).and_then(|e| e.as_ref()) {
                        if !ex.text.is_empty() {
                            let ex_text = format_example_text(ex);
                            let ex_trans = escape_html(&ex.translation);
                            if !ex_trans.is_empty() {
                                write!(out, "  <div class='ex'>{} - {}</div>\n", ex_text, ex_trans)?;
                            } else {
                                write!(out, "  <div class='ex'>{}</div>\n", ex_text)?;
                            }
                        }
                    }
                }

                if pos_count > 1 && idx < pos_count - 1 {
                    out.write_all(b"  <br/><br/>\n")?;
                }
            }
        } else {
            // English source path
            let entries_len = entries.len();
            for (idx, entry) in entries.iter().enumerate() {
                let pos = &entry.pos;
                let defs = &entry.definitions;
                let entry_examples = &entry.examples;

                let mut effective_pos = pos.clone();
                if pos == "verb" && !defs.is_empty() {
                    if defs.iter().all(|d| def_has_tag(d, "participle")) {
                        effective_pos = "participle".to_string();
                    }
                }

                let mut pos_display = self.format_pos(&effective_pos);
                if let Some(head_info) = self.get_head_info_for_pos(&[entry], word) {
                    pos_display = format!("{}, {}", pos_display, head_info);
                }
                write!(out, "  <div><i>{}</i></div>\n", escape_html(&pos_display))?;

                if defs.len() > 1 {
                    for (def_idx, definition) in defs.iter().enumerate() {
                        let clean = strip_def_qualifiers(definition);
                        write!(out, "  <div class='def'>{}. {}</div>\n", def_idx + 1, self.linkify_definition_ext(&clean, link_ext))?;
                        if let Some(ex) = entry_examples.get(def_idx).and_then(|e| e.as_ref()) {
                            if !ex.text.is_empty() {
                                let ex_text = format_example_text(ex);
                                let ex_trans = escape_html(&ex.translation);
                                if !ex_trans.is_empty() {
                                    write!(out, "  <div class='ex'>{} - {}</div>\n", ex_text, ex_trans)?;
                                } else {
                                    write!(out, "  <div class='ex'>{}</div>\n", ex_text)?;
                                }
                            }
                        }
                    }
                } else {
                    for (def_idx, definition) in defs.iter().enumerate() {
                        let clean = strip_def_qualifiers(definition);
                        write!(out, "  <div class='def'>{}</div>\n", self.linkify_definition_ext(&clean, link_ext))?;
                        if let Some(ex) = entry_examples.get(def_idx).and_then(|e| e.as_ref()) {
                            if !ex.text.is_empty() {
                                let ex_text = format_example_text(ex);
                                let ex_trans = escape_html(&ex.translation);
                                if !ex_trans.is_empty() {
                                    write!(out, "  <div class='ex'>{} - {}</div>\n", ex_text, ex_trans)?;
                                } else {
                                    write!(out, "  <div class='ex'>{}</div>\n", ex_text)?;
                                }
                            }
                        }
                    }
                }

                if let Some(etym) = &entry.etymology {
                    let trimmed = etym.trim();
                    if !trimmed.is_empty() {
                        let stripped = strip_transliterations(trimmed);
                        let cleaned = clean_etymology(&stripped);
                        if !cleaned.is_empty() {
                            write!(out, "  <div class='etym'><b>Etymology:</b> {}</div>\n", escape_html(&cleaned))?;
                        }
                    }
                }

                if entries_len > 1 && idx < entries_len - 1 {
                    out.write_all(b"  <br/><br/>\n")?;
                }
            }
        }

        Ok(())
    }

    fn rank_inflections(&self, forms: &[String]) -> Vec<String> {
        let mut tier1: Vec<(String, i64)> = Vec::new();
        let mut tier2: Vec<(String, i64)> = Vec::new();
        let mut tier3: Vec<(String, i64)> = Vec::new();

        for form in forms {
            let f = self.frequency.frequency(form);
            let c = self.dilemma.map(|d| d.confidence_for(form)).unwrap_or(0);
            if f > 0 {
                tier1.push((form.clone(), f));
            } else if c >= 3 {
                tier2.push((form.clone(), c));
            } else {
                tier3.push((form.clone(), c));
            }
        }

        tier1.sort_by(|a, b| b.1.cmp(&a.1));
        tier2.sort_by(|a, b| b.1.cmp(&a.1));
        tier3.sort_by(|a, b| b.1.cmp(&a.1));

        let mut result = Vec::with_capacity(forms.len());
        for (f, _) in tier1 { result.push(f); }
        for (f, _) in tier2 { result.push(f); }
        for (f, _) in tier3 { result.push(f); }
        result
    }

    fn select_ranked_inflections(&self, headword: &str, entry_inflections: &[String], max_count: usize) -> Vec<String> {
        if entry_inflections.is_empty() { return Vec::new(); }
        let Some(dilemma) = self.dilemma else { return entry_inflections.iter().take(max_count).cloned().collect(); };
        let Some(ranked) = dilemma.get_ranked_forms(headword) else {
            return entry_inflections.iter().take(max_count).cloned().collect();
        };

        let ranked_lower: HashSet<String> = ranked.iter().map(|f| crate::entry_processor::py_lower(f)).collect();

        let mut result: Vec<String> = Vec::new();
        let mut result_lower: HashSet<String> = HashSet::new();

        for f in entry_inflections {
            let low = crate::entry_processor::py_lower(f);
            if !ranked_lower.contains(&low) && !result_lower.contains(&low) {
                result.push(f.clone());
                result_lower.insert(low);
            }
        }
        for form in ranked {
            let low = crate::entry_processor::py_lower(form);
            if !result_lower.contains(&low) {
                result.push(form.clone());
                result_lower.insert(low);
            }
            if result.len() >= max_count { break; }
        }
        result.truncate(max_count);
        result
    }

    fn format_pos(&self, pos: &str) -> String {
        let pos_display = if pos.is_empty() { "unknown" } else { pos };
        if self.params.source_lang == "el" {
            if let Some(m) = pos_map().get(pos_display.to_lowercase().as_str()) {
                return m.to_string();
            }
        }
        pos_display.to_string()
    }

    fn get_head_info_for_pos(&self, pos_entries: &[&Entry], word: &str) -> Option<String> {
        for e in pos_entries {
            if let Some(head_exp) = &e.head_expansion {
                let stripped = strip_head_expansion(head_exp, word);
                if !stripped.is_empty() {
                    if let Some(r) = format_head_for_pos(&stripped) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }

    // Thin wrapper kept for API stability / external callers; the idx body now
    // calls linkify_definition_ext(.., "html") directly.
    #[allow(dead_code)]
    fn linkify_definition(&self, text: &str) -> String {
        // idx/MOBI path links into content_NN.html.
        self.linkify_definition_ext(text, "html")
    }

    /// Cross-reference linkifier parameterised by the target file extension so
    /// the same body markup can link into `.html` (idx/MOBI) or `.xhtml`
    /// (EPUB3) content files. Calling with "html" reproduces the historical
    /// idx output byte-for-byte.
    fn linkify_definition_ext(&self, text: &str, link_ext: &str) -> String {
        let re = greek_word_re();
        let mut out = String::new();
        let mut last = 0;
        for m in re.find_iter(text) {
            // non-match portion
            out.push_str(&escape_html(&text[last..m.start()]));
            let part = m.as_str();
            if let Some(&bucket) = self.headword_buckets.get(part) {
                let escaped = escape_html(part);
                let anchor = sanitize_anchor_id(&escaped);
                // Always file-qualify cross-reference hrefs. Entries now live
                // in per-letter content files, and we avoid threading the
                // emitting-bucket through write_entry by qualifying every link.
                out.push_str(&format!(
                    "<a href=\"{}#hw_{}\">{}</a>",
                    bucket_filename_ext(bucket, link_ext),
                    anchor,
                    escaped
                ));
            } else {
                out.push_str(&escape_html(part));
            }
            last = m.end();
        }
        out.push_str(&escape_html(&text[last..]));
        out
    }

    fn create_cover(&self) -> std::io::Result<()> {
        let override_path: Option<String> = self.params.front_matter.get("cover_path")
            .and_then(|v| v.as_str()).map(|s| s.to_string());

        // The Python code resolves relative to the lemma project root (lib/ parent).
        // We assume CWD is the lemma project root.
        let default_src = PathBuf::from("images/cover.jpg");
        let cover_src = override_path.as_ref().map(PathBuf::from).unwrap_or(default_src);
        let cover_dst = self.output_dir.join("cover.jpg");
        if cover_src.exists() {
            fs::copy(&cover_src, &cover_dst)?;
            if override_path.is_some() {
                println!("  Cover: {}", cover_src.display());
            }
        } else {
            println!("  Warning: cover image not found at {}", cover_src.display());
        }
        // Kindle renders the cover from the <meta name="cover" content="cimage"/>
        // metadata entry (the OPF 2.0 cover method; kindling falls back to it
        // when no properties="coverimage" item is present), so no HTML cover
        // page is emitted. KPG §4.2 / rule R4.2.4 explicitly warns against an
        // HTML cover page in addition to the cover image.
        Ok(())
    }

    fn create_copyright_html(&self) -> std::io::Result<()> {
        let fm = &self.params.front_matter;
        let year = Local::now().year();

        let holder = fm.pointer("/copyright/holder").and_then(|v| v.as_str()).unwrap_or("Open Greek");
        let empty_vec: Vec<Value> = Vec::new();
        let extra_lines = fm.pointer("/copyright/extra_lines").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
        let tools = fm.get("tools").and_then(|v| v.as_array()).unwrap_or(&empty_vec);
        let data_sources = fm.get("data_sources").and_then(|v| v.as_array()).unwrap_or(&empty_vec);

        let created_fmt = format_created_date(&self.params.build_date);
        let extraction_fmt = format_extraction_date(self.params.extraction_date.as_deref());

        let mut lines: Vec<String> = Vec::new();
        lines.push("    <h2>Copyright</h2>".to_string());
        lines.push(format!("    \u{00a9} {} {}. All rights reserved.<br />", year, escape_html(holder)));
        lines.push("    <br />".to_string());
        for tool in tools {
            let role = escape_html(tool.get("role").and_then(|v| v.as_str()).unwrap_or(""));
            let name = escape_html(tool.get("name").and_then(|v| v.as_str()).unwrap_or(""));
            let url = tool.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let suffix = escape_html(tool.get("suffix").and_then(|v| v.as_str()).unwrap_or("."));
            let anchor = if !url.is_empty() {
                format!("<a href=\"{}\">{}</a>", escape_html(url), name)
            } else {
                name.clone()
            };
            lines.push(format!("    {} {}{}<br />", role, anchor, suffix));
            lines.push("    <br />".to_string());
        }
        for extra in extra_lines {
            if let Some(s) = extra.as_str() {
                lines.push(format!("    {}<br />", escape_html(s)));
                lines.push("    <br />".to_string());
            }
        }

        lines.push("    <h2>Data Sources</h2>".to_string());
        for src in data_sources {
            let name = escape_html(src.get("name").and_then(|v| v.as_str()).unwrap_or(""));
            let url = src.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let desc = src.get("desc").and_then(|v| v.as_str()).unwrap_or("");
            let anchor = if !url.is_empty() {
                format!("<a href=\"{}\">{}</a>", escape_html(url), name)
            } else {
                name.clone()
            };
            lines.push(format!("    {}: {}<br />", anchor, desc));
            lines.push("    <br />".to_string());
        }

        lines.push("    <h2>Build Info</h2>".to_string());
        lines.push(format!("    Wiktionary data extracted: {}<br />", escape_html(&extraction_fmt)));
        lines.push(format!("    Dictionary created: {}<br />", escape_html(&created_fmt)));
        lines.push(format!(
            "    Generator: lemma v{}<br />",
            escape_html(crate::version::LEMMA_VERSION)
        ));

        let body = lines.join("\n");
        // XHTML 1.0 forbids raw text and inline elements (<br/>, <a>, etc.)
        // as direct children of <body>. Wrap the existing <br/>-separated
        // content in a single <div> so strict validators stop rejecting it.
        // <head> must also contain a <title>.
        let content = format!(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\">\n  <head>\n    <title>Copyright</title>\n    <meta content=\"text/html; charset=utf-8\" http-equiv=\"content-type\" />\n  </head>\n  <body>\n    <div>\n{}\n    </div>\n  </body>\n</html>\n",
            body
        );
        let mut f = File::create(self.output_dir.join("copyright.html"))?;
        f.write_all(content.as_bytes())?;
        Ok(())
    }

    fn create_usage_html(&self) -> std::io::Result<()> {
        let fm = &self.params.front_matter;
        let default_name = self.default_edition_name();
        let dict_name_s = fm.get("edition_name").and_then(|v| v.as_str()).unwrap_or(&default_name);
        let tagline_default = "A Greek-English dictionary built from English Wiktionary. Look up any Greek word while reading, inflected forms automatically redirect to their headword.";
        let tagline = fm.get("tagline").and_then(|v| v.as_str()).unwrap_or(tagline_default);
        let empty_vec: Vec<Value> = Vec::new();
        let features = fm.get("features").and_then(|v| v.as_array()).unwrap_or(&empty_vec);

        let feature_items: Vec<String> = features
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| format!("      <li>{}</li>", escape_html(s)))
            .collect();
        let features_block = if !feature_items.is_empty() {
            format!("    <ul>\n{}\n    </ul>", feature_items.join("\n"))
        } else {
            String::new()
        };

        // XHTML 1.0 forbids inline children (<span>, <br/>) directly under
        // <body>. Wrap the tagline in <p>, drop the stray <br/> spacers that
        // sat between block-level headings, keep the <style> element with an
        // explicit type attribute, and add the required <title>.
        let escaped_name = escape_html(dict_name_s);
        let content = format!(
            "<html xmlns=\"http://www.w3.org/1999/xhtml\">\n  <head>\n    <title>{name}</title>\n    <meta content=\"text/html; charset=utf-8\" http-equiv=\"content-type\" />\n    <style type=\"text/css\">p {{ text-indent: 0; margin: 0.3em 0; }}</style>\n  </head>\n  <body>\n    <h2>{name}</h2>\n    <p>{tagline}</p>\n    <h3>Features</h3>\n{features}\n    <h3>To Set as Default Greek Dictionary</h3>\n    <ul>\n      <li>Look up any Greek word in your book</li>\n      <li>Tap the dictionary name in the popup</li>\n      <li>Select \"{name}\"</li>\n    </ul>\n  </body>\n</html>\n",
            name = escaped_name,
            tagline = escape_html(tagline),
            features = features_block,
        );
        let mut f = File::create(self.output_dir.join("usage.html"))?;
        f.write_all(content.as_bytes())?;
        Ok(())
    }

    fn default_edition_name(&self) -> String {
        // Distinguishes this fork's builds from the upstream open-greek/lemma
        // release in Kindle's Settings -> Language & Dictionaries picker,
        // which reads this from <dc:title> / the "dictionary-name" meta
        // below - both driven by this string.
        let direction = if self.params.source_lang == "en" { "Greek-English" } else { "Greek-Greek" };
        format!("Lemma {} (v3)", direction)
    }

    fn create_opf_file(&mut self) -> std::io::Result<()> {
        let source_name = if self.params.source_lang == "en" { "en-el" } else { "el-el" };
        // Stable unique identifier. NO date suffix and NO edition tag -
        // lemma now ships a single unified edition, so Kindle treats new
        // versions as in-place upgrades not fresh dictionaries.
        let unique_id = format!(
            "LemmaGreek{}",
            source_name.to_uppercase().replace('-', "")
        );
        let display_title = self.params.front_matter
            .get("edition_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.default_edition_name());
        let out_lang = if self.params.source_lang == "en" { "en" } else { "el" };
        let opf_filename = format!("lemma_greek_{}.opf", self.params.source_lang);

        let extraction = self.params.extraction_date.clone().unwrap_or_else(|| "Unknown".to_string());

        // OPF 2.0 <dc:date> must follow W3CDTF (ISO 8601). build_date is
        // YYYYMMDD elsewhere in the pipeline; convert to YYYY-MM-DD here.
        let iso_build_date = if self.params.build_date.len() == 8
            && self.params.build_date.chars().all(|c| c.is_ascii_digit())
        {
            format!(
                "{}-{}-{}",
                &self.params.build_date[..4],
                &self.params.build_date[4..6],
                &self.params.build_date[6..8]
            )
        } else {
            self.params.build_date.clone()
        };

        // Content is split across one XHTML per Greek letter (plus a "misc"
        // bucket for non-Greek headwords). Manifest and spine loop over the
        // buckets that actually received entries during content generation.
        let content_manifest = self
            .buckets_used
            .iter()
            .map(|&b| {
                format!(
                    "    <item id=\"{id}\"\n          href=\"{href}\"\n          media-type=\"application/xhtml+xml\" />",
                    id = bucket_id(b),
                    href = bucket_filename(b)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let content_spine = self
            .buckets_used
            .iter()
            .map(|&b| format!("    <itemref idref=\"{}\"/>", bucket_id(b)))
            .collect::<Vec<_>>()
            .join("\n");
        let first_content_href = self
            .buckets_used
            .first()
            .map(|&b| bucket_filename(b))
            .unwrap_or_else(|| "content_00.html".to_string());

        let content = format!(
r#"<?xml version="1.0" encoding="UTF-8"?>
<package version="2.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="BookId">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>{title}</dc:title>
    <dc:creator opf:role="aut">Open Greek</dc:creator>
    <dc:language>el</dc:language>
    <dc:publisher>Lemma</dc:publisher>
    <dc:rights>Creative Commons Attribution-ShareAlike 4.0 International</dc:rights>
    <dc:date>{build_date}</dc:date>
    <dc:identifier id="BookId">{uid}</dc:identifier>
    <meta name="cover" content="cimage" />
    <meta name="wiktionary-extraction-date" content="{extraction}" />
    <meta name="dictionary-name" content="{title}" />
    <meta name="generator" content="lemma" />
    <meta name="generator-version" content="{version}" />
    <x-metadata>
      <DictionaryInLanguage>el</DictionaryInLanguage>
      <DictionaryOutLanguage>{outlang}</DictionaryOutLanguage>
      <DefaultLookupIndex>default</DefaultLookupIndex>
    </x-metadata>
  </metadata>
  <manifest>
    <item id="ncx"
          href="toc.ncx"
          media-type="application/x-dtbncx+xml" />
    <item id="cimage"
          href="cover.jpg"
          media-type="image/jpeg" />
    <item id="usage"
          href="usage.html"
          media-type="application/xhtml+xml" />
    <item id="copyright"
          href="copyright.html"
          media-type="application/xhtml+xml" />
{content_manifest}
  </manifest>
  <spine toc="ncx">
    <itemref idref="usage" />
    <itemref idref="copyright"/>
{content_spine}
  </spine>
  <guide>
    <reference type="index" title="Dictionary" href="{first_content_href}"/>
  </guide>
</package>
"#,
            title = display_title,
            build_date = iso_build_date,
            uid = unique_id,
            extraction = extraction,
            outlang = out_lang,
            version = crate::version::LEMMA_VERSION,
            content_manifest = content_manifest,
            content_spine = content_spine,
            first_content_href = first_content_href,
        );

        let opf_path = self.output_dir.join(&opf_filename);
        let mut f = File::create(&opf_path)?;
        f.write_all(content.as_bytes())?;
        self.opf_filename = opf_filename;
        Ok(())
    }

    fn create_toc_ncx(&self) -> std::io::Result<()> {
        let source_name = if self.params.source_lang == "en" { "en-el" } else { "el-el" };
        let unique_id = format!("LemmaGreek{}", source_name.to_uppercase().replace('-', ""));
        let display_title = self.params.front_matter
            .get("edition_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.default_edition_name());

        // Emit one navPoint per letter (pointing at that letter's first file)
        // so the Kindle TOC offers jump-to-letter navigation even when a letter
        // is split across several content files.
        let letter_navpoints = self
            .nav_points
            .iter()
            .enumerate()
            .map(|(i, (file_idx, label))| {
                let play_order = 3 + i;
                format!(
                    "    <navPoint id=\"{id}\" playOrder=\"{play_order}\">\n      <navLabel><text>{label}</text></navLabel>\n      <content src=\"{href}\"/>\n    </navPoint>",
                    id = bucket_id(*file_idx),
                    play_order = play_order,
                    label = label,
                    href = bucket_filename(*file_idx)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let content = format!(
r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd">
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="{uid}"/>
    <meta name="dtb:depth" content="1"/>
    <meta name="dtb:totalPageCount" content="0"/>
    <meta name="dtb:maxPageNumber" content="0"/>
  </head>
  <docTitle>
    <text>{title}</text>
  </docTitle>
  <navMap>
    <navPoint id="usage" playOrder="1">
      <navLabel><text>Usage</text></navLabel>
      <content src="usage.html"/>
    </navPoint>
    <navPoint id="copyright" playOrder="2">
      <navLabel><text>Copyright</text></navLabel>
      <content src="copyright.html"/>
    </navPoint>
{letter_navpoints}
  </navMap>
</ncx>
"#,
            uid = unique_id,
            title = display_title,
            letter_navpoints = letter_navpoints,
        );
        let mut f = File::create(self.output_dir.join("toc.ncx"))?;
        f.write_all(content.as_bytes())?;
        Ok(())
    }
}

// --- Free helpers ---

fn html_header(title: &str) -> String {
    // Header layout matches KPG §15.3.2 exactly. Do NOT add
    // xmlns="http://www.w3.org/1999/xhtml" as a default namespace here:
    // Amazon's dictionary converter is built to parse the unnamespaced
    // KPG example verbatim, and adding a default XHTML namespace puts
    // every unprefixed element into the XHTML namespace, which upstream
    // dictionary indexing pipelines do not recognize. <title> + style
    // type="text/css" stay in because they are harmless in HTML mode and
    // help any downstream tooling that happens to parse strictly.
    format!(
        "<html xmlns:math=\"http://exslt.org/math\" xmlns:svg=\"http://www.w3.org/2000/svg\"\n      xmlns:tl=\"https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf\"\n      xmlns:saxon=\"http://saxon.sf.net/\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"\n      xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"\n      xmlns:cx=\"https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf\"\n      xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n      xmlns:mbp=\"https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf\"\n      xmlns:mmc=\"https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf\"\n      xmlns:idx=\"https://kindlegen.s3.amazonaws.com/AmazonKindlePublishingGuidelines.pdf\">\n  <head>\n    <title>{}</title>\n    <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\" />\n    <style type=\"text/css\">\n      h5 {{ font-size: 1em; margin: 0; }}\n      div {{ margin: 0.2em 0; }}\n      b {{ font-weight: bold; }}\n      i {{ font-style: italic; }}\n      .pos {{ font-style: italic; }}\n      .def {{ margin-left: 20px; }}\n      .ex {{ margin-left: 20px; }}\n      .etym {{ margin-top: 0.5em; margin-left: 0; background-color: #f0f0f0; padding: 0.2em 0.4em; }}\n      hr {{ margin: 5px 0; border: none; border-top: 1px solid #ccc; }}\n    </style>\n  </head>\n  <body>\n    <mbp:frameset>\n",
        escape_html(title)
    )
}

fn html_footer() -> &'static str {
    "    </mbp:frameset>\n  </body>\n</html>\n"
}

fn sanitize_anchor_id(text: &str) -> String {
    text.replace(' ', "_")
}

/// Compute the letter-bucket index for a headword. Returns 1..=24 for
/// Α..Ω (accent-insensitive, case-insensitive) and 0 for any non-Greek
/// first character (numerals, archaic numerals, Latin abbreviations, etc.).
fn bucket_for_headword(word: &str) -> u8 {
    let normalized = normalize_for_sorting(word);
    let Some(first) = normalized.chars().next() else { return 0; };
    match first {
        'α' => 1,  'β' => 2,  'γ' => 3,  'δ' => 4,  'ε' => 5,
        'ζ' => 6,  'η' => 7,  'θ' => 8,  'ι' => 9,  'κ' => 10,
        'λ' => 11, 'μ' => 12, 'ν' => 13, 'ξ' => 14, 'ο' => 15,
        'π' => 16, 'ρ' => 17, 'σ' | 'ς' => 18, 'τ' => 19, 'υ' => 20,
        'φ' => 21, 'χ' => 22, 'ψ' => 23, 'ω' => 24,
        _ => 0,
    }
}

fn bucket_filename(bucket: u8) -> String {
    bucket_filename_ext(bucket, "html")
}

/// Like `bucket_filename` but with an explicit extension. The idx/MOBI path
/// uses "html"; the EPUB3 path uses "xhtml". Keeping a single formatter
/// guarantees the bucket numbering stays consistent across both outputs.
fn bucket_filename_ext(bucket: u8, ext: &str) -> String {
    format!("content_{:02}.{}", bucket, ext)
}

fn bucket_id(bucket: u8) -> String {
    format!("content_{:02}", bucket)
}

fn bucket_label(bucket: u8) -> &'static str {
    match bucket {
        1 => "Α", 2 => "Β", 3 => "Γ", 4 => "Δ", 5 => "Ε",
        6 => "Ζ", 7 => "Η", 8 => "Θ", 9 => "Ι", 10 => "Κ",
        11 => "Λ", 12 => "Μ", 13 => "Ν", 14 => "Ξ", 15 => "Ο",
        16 => "Π", 17 => "Ρ", 18 => "Σ", 19 => "Τ", 20 => "Υ",
        21 => "Φ", 22 => "Χ", 23 => "Ψ", 24 => "Ω",
        _ => "Other",
    }
}

fn normalize_for_sorting(word: &str) -> String {
    // lowercase (Greek final-sigma aware), strip accents, remove non Greek/Latin/digit
    let lower = crate::entry_processor::py_lower(word);
    let stripped: String = lower.chars().map(|c| match c {
        'ά' => 'α', 'έ' => 'ε', 'ή' => 'η', 'ί' => 'ι', 'ό' => 'ο', 'ύ' => 'υ', 'ώ' => 'ω',
        'ΐ' => 'ι', 'ΰ' => 'υ', 'ϊ' => 'ι', 'ϋ' => 'υ',
        'Ά' => 'α', 'Έ' => 'ε', 'Ή' => 'η', 'Ί' => 'ι', 'Ό' => 'ο', 'Ύ' => 'υ', 'Ώ' => 'ω',
        _ => c,
    }).collect();
    re_non_sort_char().replace_all(&stripped, "").into_owned()
}

fn re_paren_prefix() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\(([^)]+)\)").unwrap())
}
fn re_paren_prefix_ws() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\(([^)]+)\)\s*").unwrap())
}
fn re_strip_translit_a() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\s*\([^)]*[\u00C0-\u024F\u1E00-\u1EFF][^)]*\)").unwrap())
}
fn re_strip_translit_b() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"\s*\([^)"]*"[^)]*\)"#).unwrap())
}
fn re_head_prefix() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^\([A-Za-z\u00C0-\u024F\s]+\)\s*").unwrap())
}
fn re_head_format() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^([mfn](?:\s+or\s+[mfn])*)(\s+(?:pl|sg))?(\s+\(.*\))?\s*$").unwrap())
}
fn re_non_sort_char() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"[^\u0370-\u03FF\u1F00-\u1FFFA-Za-z0-9]").unwrap())
}
fn gender_word_re(ab: &str) -> &'static Regex {
    static M: OnceLock<Regex> = OnceLock::new();
    static F: OnceLock<Regex> = OnceLock::new();
    static N: OnceLock<Regex> = OnceLock::new();
    match ab {
        "m" => M.get_or_init(|| Regex::new(r"\bm\b").unwrap()),
        "f" => F.get_or_init(|| Regex::new(r"\bf\b").unwrap()),
        _ => N.get_or_init(|| Regex::new(r"\bn\b").unwrap()),
    }
}

fn def_has_tag(definition: &str, tag: &str) -> bool {
    match re_paren_prefix().captures(definition) {
        Some(c) => {
            let group = c.get(1).unwrap().as_str();
            group.split(',').any(|t| t.trim().to_lowercase() == tag)
        }
        None => false,
    }
}

fn strip_def_qualifiers(definition: &str) -> String {
    let stripped = strip_transliterations(definition);
    match re_paren_prefix_ws().captures(&stripped) {
        None => stripped,
        Some(c) => {
            let group = c.get(1).unwrap().as_str();
            let tags: Vec<&str> = group.split(',').map(|t| t.trim()).collect();
            let remaining: Vec<&&str> = tags.iter().filter(|t| !strip_tags().contains(t.to_lowercase().as_str())).collect();
            let end = c.get(0).unwrap().end();
            let rest = &stripped[end..];
            if !remaining.is_empty() {
                let joined: Vec<&str> = remaining.iter().map(|s| **s).collect();
                format!("({}) {}", joined.join(", "), rest)
            } else {
                rest.to_string()
            }
        }
    }
}

fn strip_transliterations(text: &str) -> String {
    let t1 = re_strip_translit_a().replace_all(text, "").into_owned();
    re_strip_translit_b().replace_all(&t1, "").into_owned()
}

fn clean_etymology(text: &str) -> String {
    let mut out = text.to_string();
    for marker in &["Typological comparisons", "See also", "\n*", "\n•"] {
        if let Some(pos) = out.find(marker) {
            if pos > 0 {
                out.truncate(pos);
            }
        }
    }
    out.trim().to_string()
}

fn strip_head_expansion(head_exp: &str, word: &str) -> String {
    let mut text = head_exp.to_string();
    if let Some(bullet_pos) = text.find('•') {
        text = text[bullet_pos + '•'.len_utf8()..].trim().to_string();
    } else if text.starts_with(word) {
        text = text[word.len()..].trim().to_string();
    }
    re_head_prefix().replace(&text, "").trim().to_string()
}

fn format_head_for_pos(stripped: &str) -> Option<String> {
    if stripped.is_empty() { return None; }
    let caps = re_head_format().captures(stripped)?;
    let mut genders = caps.get(1)?.as_str().to_string();
    let number = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
    let parens = caps.get(3).map(|m| m.as_str().trim()).unwrap_or("");

    // Substitute m/f/n gender codes with their full names, as word-boundary
    // matches so "or" between them isn't touched.
    for (ab, full) in &[("m", "masculine"), ("f", "feminine"), ("n", "neuter")] {
        let re = gender_word_re(ab);
        genders = re.replace_all(&genders, *full).into_owned();
    }
    let mut parts = vec![genders];
    if !number.is_empty() {
        parts.push(if number == "pl" { "plural".to_string() } else { "singular".to_string() });
    }
    let mut result = parts.join(" ");
    if !parens.is_empty() {
        result.push(' ');
        result.push_str(parens);
    }
    Some(result)
}

fn format_example_text(ex: &Example) -> String {
    let text = &ex.text;
    let Some(offsets) = &ex.bold_text_offsets else {
        return escape_html(text);
    };
    // Offsets from Kaikki are Python str indices = codepoint offsets. Convert to byte
    // offsets over the current working string on each replacement so insertion markers
    // don't shift the subsequent offsets.
    let mut sorted: Vec<(usize, usize)> = offsets.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    let mut working = text.clone();
    for (start, end) in sorted {
        let byte_start = match codepoint_to_byte(&working, start) {
            Some(b) => b,
            None => continue,
        };
        let byte_end = match codepoint_to_byte(&working, end) {
            Some(b) => b,
            None => continue,
        };
        if byte_start > byte_end { continue; }
        working = format!(
            "{}\u{0001}{}\u{0002}{}",
            &working[..byte_start],
            &working[byte_start..byte_end],
            &working[byte_end..]
        );
    }
    let escaped = escape_html(&working);
    escaped.replace('\u{0001}', "<b>").replace('\u{0002}', "</b>")
}

fn codepoint_to_byte(s: &str, cp: usize) -> Option<usize> {
    // Python str slicing clamps out-of-range indices to the end; we mirror that.
    if cp == 0 { return Some(0); }
    for (i, (byte_idx, _)) in s.char_indices().enumerate() {
        if i == cp { return Some(byte_idx); }
    }
    Some(s.len())
}

fn format_created_date(raw: &str) -> String {
    if raw.len() == 8 && raw.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(raw, "%Y%m%d") {
            let month = month_name(dt.month());
            return format!("{} {}, {}", month, dt.day(), dt.year());
        }
    }
    if raw.is_empty() { "Unknown".to_string() } else { raw.to_string() }
}

fn format_extraction_date(raw: Option<&str>) -> String {
    let Some(raw) = raw else { return "Unknown".to_string(); };
    for pat in &["%Y-%m-%d", "%Y-%m-%dT%H:%M:%S", "%Y%m%d"] {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(raw, pat) {
            return format!("{} {}, {}", month_name(dt.month()), dt.day(), dt.year());
        }
        // Try DateTime parse for ISO-with-time
        if *pat == "%Y-%m-%dT%H:%M:%S" {
            if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(raw, pat) {
                return format!("{} {}, {}", month_name(dt.month()), dt.day(), dt.year());
            }
        }
    }
    raw.to_string()
}

fn month_name(m: u32) -> &'static str {
    match m {
        1 => "January", 2 => "February", 3 => "March", 4 => "April",
        5 => "May", 6 => "June", 7 => "July", 8 => "August",
        9 => "September", 10 => "October", 11 => "November", 12 => "December",
        _ => "",
    }
}

// --- Polytonic variant generation ---

fn breathing_variants(form: &str) -> Vec<String> {
    if form.is_empty() { return Vec::new(); }
    let chars: Vec<char> = form.chars().collect();
    if !breathing().contains_key(&chars[0]) { return Vec::new(); }

    if chars.len() >= 2 && DIPHTHONG_FIRSTS.contains(chars[0]) && DIPHTHONG_SECONDS.contains(chars[1]) {
        let Some((smooth, rough)) = breathing().get(&chars[1]).copied() else { return Vec::new(); };
        let rest: String = chars[2..].iter().collect();
        return vec![
            format!("{}{}{}", chars[0], smooth, rest),
            format!("{}{}{}", chars[0], rough, rest),
        ];
    }
    let Some((smooth, rough)) = breathing().get(&chars[0]).copied() else { return Vec::new(); };
    let rest: String = chars[1..].iter().collect();
    vec![format!("{}{}", smooth, rest), format!("{}{}", rough, rest)]
}

fn accent_variants(form: &str) -> Vec<String> {
    let mut results = Vec::new();
    let chars: Vec<char> = form.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if acute_to_grave().contains_key(c) {
            if let Some(grave) = acute_to_grave().get(c).copied() {
                let mut new_chars = chars.clone();
                new_chars[i] = grave;
                results.push(new_chars.into_iter().collect());
            }
            if let Some(circ) = acute_to_circumflex().get(c).copied() {
                let mut new_chars = chars.clone();
                new_chars[i] = circ;
                results.push(new_chars.into_iter().collect());
            }
            break;
        }
    }
    results
}

fn polytonic_variants(form: &str) -> Vec<String> {
    let mut results: HashSet<String> = HashSet::new();
    for bv in breathing_variants(form) {
        results.insert(bv);
    }
    for av in accent_variants(form) {
        for bv in breathing_variants(&av) {
            results.insert(bv);
        }
        results.insert(av);
    }
    results.remove(form);
    results.into_iter().collect()
}

#[cfg(test)]
mod lookup_coverage_tests {
    use super::*;
    use crate::entry_processor::{Entry, EntryMap};

    fn params(punct_variants: usize) -> BuildParams {
        BuildParams {
            source_lang: "el".to_string(),
            build_date: "20260101".to_string(),
            extraction_date: None,
            limit_percent: None,
            max_inflections: None,
            punct_variants,
            front_matter: serde_json::json!({}),
        }
    }

    fn noun_entry(inflections: &[&str]) -> Entry {
        Entry {
            pos: "noun".to_string(),
            inflections: inflections.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn entry_variations_includes_enclitic_double_accent() {
        let mut entries = EntryMap::new();
        entries.insert(
            "χαρτοφύλακας".to_string(),
            vec![noun_entry(&["χαρτοφύλακα", "χαρτοφύλακες"])],
        );
        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let entries_ref = htmlgen.entries.get(&"χαρτοφύλακας".to_string()).unwrap().clone();
        let variations = htmlgen.entry_variations("χαρτοφύλακας", &entries_ref);

        assert!(
            variations.iter().any(|v| v == "χαρτοφύλακά"),
            "expected enclitic double-accent variant, got {:?}",
            variations
        );
    }

    #[test]
    fn entry_variations_includes_punctuation_variants_when_enabled() {
        let mut entries = EntryMap::new();
        entries.insert("λέξη".to_string(), vec![noun_entry(&[])]);
        let mut htmlgen = HtmlGenerator::new(entries, params(5), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let entries_ref = htmlgen.entries.get(&"λέξη".to_string()).unwrap().clone();
        let variations = htmlgen.entry_variations("λέξη", &entries_ref);

        for expected in ["λέξη,", "λέξη.", "«λέξη", "λέξη\u{0387}"] {
            assert!(
                variations.iter().any(|v| v == expected),
                "expected {:?} in {:?}",
                expected,
                variations
            );
        }
    }

    #[test]
    fn entry_variations_excludes_punctuation_variants_when_disabled() {
        let mut entries = EntryMap::new();
        entries.insert("λέξη".to_string(), vec![noun_entry(&[])]);
        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let entries_ref = htmlgen.entries.get(&"λέξη".to_string()).unwrap().clone();
        let variations = htmlgen.entry_variations("λέξη", &entries_ref);

        assert!(!variations.iter().any(|v| v == "λέξη,"));
    }

    #[test]
    fn double_accent_skips_collision_with_real_distinct_headword() {
        // "χαρτοφύλακας" has the proparoxytone inflection "χαρτοφύλακα",
        // whose enclitic double-accent transform is "χαρτοφύλακά". Here that
        // exact string also exists as a real, separate dictionary headword -
        // the variant must be dropped, not silently pointed at the wrong
        // entry.
        let mut entries = EntryMap::new();
        entries.insert(
            "χαρτοφύλακας".to_string(),
            vec![noun_entry(&["χαρτοφύλακα"])],
        );
        entries.insert("χαρτοφύλακά".to_string(), vec![noun_entry(&[])]);

        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        assert!(
            htmlgen.double_accent_variants
                .get("χαρτοφύλακας")
                .map(|v| !v.contains(&"χαρτοφύλακά".to_string()))
                .unwrap_or(true),
            "double-accent variant colliding with a real headword must be skipped"
        );
    }

    #[test]
    fn punct_variants_resolve_cross_headword_collision_by_frequency() {
        // "καλά" is both a headword (adverb) and an inflected form of
        // "καλός" (neuter plural adjective) - ordinary Greek syncretism.
        // Both independently produce the punctuation-attached candidate
        // "καλά," at N>0. Without cross-headword resolution (the bug this
        // test guards against), the same string would end up emitted under
        // BOTH headwords' <idx:infl> blocks, an ambiguous/undefined lookup
        // for Kindle. It must land on exactly one.
        let mut entries = EntryMap::new();
        entries.insert("καλά".to_string(), vec![noun_entry(&[])]);
        entries.insert("καλός".to_string(), vec![noun_entry(&["καλά"])]);

        let mut htmlgen = HtmlGenerator::new(entries, params(5), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let owns_from_kala = htmlgen
            .punct_variants_map
            .get("καλά")
            .map(|v| v.contains(&"καλά,".to_string()))
            .unwrap_or(false);
        let owns_from_kalos = htmlgen
            .punct_variants_map
            .get("καλός")
            .map(|v| v.contains(&"καλά,".to_string()))
            .unwrap_or(false);

        assert!(
            owns_from_kala ^ owns_from_kalos,
            "\"καλά,\" must be claimed by exactly one of the two colliding headwords: from_kala={}, from_kalos={}",
            owns_from_kala,
            owns_from_kalos
        );
    }

    #[test]
    fn capitalized_variants_close_indeclinable_gap() {
        // "όχι" (an interjection) has no Wiktionary `forms` array, so the
        // old capitalize_first() call inside entry_processor.rs's forms
        // loop never ran for it. build_capitalized_variants is scoped to
        // headword + inflections unconditionally, so it must produce a
        // capitalized variant for a bare headword with zero inflections.
        let mut entries = EntryMap::new();
        entries.insert("όχι".to_string(), vec![noun_entry(&[])]);

        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let entries_ref = htmlgen.entries.get(&"όχι".to_string()).unwrap().clone();
        let variations = htmlgen.entry_variations("όχι", &entries_ref);

        assert!(
            variations.iter().any(|v| v == "Όχι"),
            "expected accent-preserving capital 'Όχι', got {:?}",
            variations
        );
        assert!(
            variations.iter().any(|v| v == "Οχι"),
            "expected accent-dropped capital 'Οχι', got {:?}",
            variations
        );
    }

    #[test]
    fn capitalized_variants_skip_accent_drop_when_first_letter_unaccented() {
        // Consonant-initial words only get one capitalized variant - the
        // accent-preserving and accent-dropped forms would be identical, so
        // the second must be skipped rather than emitted as a duplicate.
        let mut entries = EntryMap::new();
        entries.insert("μα".to_string(), vec![noun_entry(&[])]);

        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let entries_ref = htmlgen.entries.get(&"μα".to_string()).unwrap().clone();
        let variations = htmlgen.entry_variations("μα", &entries_ref);

        let cap_count = variations.iter().filter(|v| v.as_str() == "Μα").count();
        assert_eq!(cap_count, 1, "expected exactly one 'Μα' variant, got {:?}", variations);
    }

    #[test]
    fn capitalized_variants_cover_inflections_not_just_headword() {
        let mut entries = EntryMap::new();
        entries.insert(
            "δείχνω".to_string(),
            vec![noun_entry(&["έδειχναν"])],
        );

        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        let entries_ref = htmlgen.entries.get(&"δείχνω".to_string()).unwrap().clone();
        let variations = htmlgen.entry_variations("δείχνω", &entries_ref);

        assert!(variations.iter().any(|v| v == "Έδειχναν"), "got {:?}", variations);
        assert!(variations.iter().any(|v| v == "Εδειχναν"), "got {:?}", variations);
    }

    #[test]
    fn capitalized_variants_skip_collision_with_real_distinct_headword() {
        let mut entries = EntryMap::new();
        entries.insert("όχι".to_string(), vec![noun_entry(&[])]);
        entries.insert("Όχι".to_string(), vec![noun_entry(&[])]);

        let mut htmlgen = HtmlGenerator::new(entries, params(0), None);
        htmlgen.merge_form_of_into_parents();
        htmlgen.build_iform_owners();
        htmlgen.build_double_accent_variants();
        htmlgen.build_punct_variants();
        htmlgen.build_capitalized_variants();

        assert!(
            htmlgen.capitalized_variants
                .get("όχι")
                .map(|v| !v.contains(&"Όχι".to_string()))
                .unwrap_or(true),
            "capitalized variant colliding with a real distinct headword must be skipped"
        );
    }
}
