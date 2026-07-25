# Lookup Coverage Session Progress

Branch: `claude/lemma-lookup-coverage-kddi2k`

## Shipped (committed and pushed)

- **Feature 1: enclitic double-accentuation** (`src/accent.rs`) — always on, no CLI
  flag. From-scratch Modern Greek syllabifier (diphthong merging + ι-synizesis)
  flags proparoxytone headwords/inflections and indexes their enclitic-attached
  spelling (χαρτοφύλακα → χαρτοφύλακά). Dictionary-wide collision resolution
  (`build_double_accent_variants` in `src/html_gen.rs`): a candidate colliding
  with a real distinct headword is dropped and logged; a candidate independently
  produced by two headwords is resolved by frequency. Verified against the real
  ~31K-headword build: **234,585 variants added across 12,280 headwords, 6,777
  contested resolved by frequency, 0 real collisions.**

- **Feature 2: punctuation variants** (`src/punctuation.rs`, `--punct-variants N`,
  **default N=1**). Indexes the headword plus its top-N ranked inflections with
  trailing/leading punctuation (λέξη, / λέξη. / «λέξη), plus stacked
  closing-quote+sentence-mark trailing combinations (CLOSING × SENTENCE =
  παλιόπαιδο»,-style) for the same scope. Includes the cross-headword
  collision-resolution fix (originally missing — Feature 2 used to dedupe only
  within one headword's own list; found via real-data testing that this
  silently produced ~99K unresolved duplicate strings across different
  headwords at N=20). `build_punct_variants` now mirrors Feature 1's
  dictionary-wide resolution exactly. N=1 was chosen after measuring real
  content/MOBI size at N=1/3/5/20 (cost scales ~linearly per added ranked form,
  no economy of scale favoring higher N).

- **Dictionary title** updated to distinguish this fork's builds from upstream
  in Kindle's Settings → Language & Dictionaries picker: `default_edition_name()`
  in `src/html_gen.rs` (plus the matching startup-preview fallback in
  `src/generator.rs`) now returns `"Lemma Greek-English (v3)"` for `--source en`
  builds (`"Lemma Greek-Greek (v3)"` for `--source el`). Verified at the OPF,
  generated content, and compiled `.mobi` (EXTH metadata + PalmDB header name)
  levels.

Commits: `f4a609c`, `c9a50a6`, `13d90ad`, `345b060`.

## Known, deliberately out-of-scope

- **Productive compounding** (e.g. καστανόχρωμη) — open-vocabulary segmentation
  + gloss synthesis, explicitly deferred per the original spec.
- **Synonym-only words like παλιόπαιδο** — never a real Wiktionary headword in
  this dataset; only appears nested inside other entries' `senses[].synonyms[]`
  lists, which `entry_processor.rs` doesn't currently harvest into new
  headwords. Data gap, not a code bug. (Diagnosed at length — this is where the
  `Όχι` investigation started before finding the *actual* separate
  capitalization bug below.)

## Known, real, NOT yet fixed

- **Indeclinable-word capitalization gap** (Όχι, Μα, Έτσι, etc.) — the bare
  capitalized form of a headword is never generated as an iform, because the
  capitalize-first/lower-first logic in `entry_processor.rs` (~line 307-333)
  only runs *inside* the loop over the raw Wiktionary `forms` array. Indeclinable
  words (interjections, invariant particles/adverbs) have an empty `forms`
  array, so the loop body never executes and no capitalized variant is ever
  produced for the headword itself — independent of punctuation, independent of
  everything else. Root cause confirmed directly against `EntryProcessor`
  output for `όχι`, `μα`, `έτσι`. Not yet fixed.

- **ALL-CAPS word gap** — if `strict_accents: true` ships (see below), this is a
  new, *accepted* tradeoff, not a surprise: `entry_processor.rs` never generates
  full-uppercase variants (only first-letter-capitalized and all-lowercase), so
  any book text using full-caps styling (headings, emphasis) was only ever
  reachable via device-side case folding — which is what `strict_accents: true`
  disables.

## Currently mid-decision (`strict_accents: true` committed as a TEST flag, not a decision)

- **`strict_accents: true` in `src/mobi.rs`** was committed (`08a575c`) with an
  explicit "temporary test flag, not yet a final decision" message — see
  on-device result below. It is **not reverted yet**, pending the
  investigation in this section.

- **Original theory (now falsified by on-device evidence):** kindling sorts
  the Greek INDX by raw UTF-16BE byte value, which places accented-capital
  letters (U+0386–U+038F) in a disjoint region from their lowercase
  counterparts (U+03B1+). With `strict_accents: false` (default), kindling
  embeds a diacritic-folding collation table, so the device searches using a
  different (folded) comparison than the one the file is actually sorted
  with — theorized to make capitalized entries unreachable via binary search
  even though correctly present in the file. `strict_accents: true` was
  supposed to fix this by omitting the folding table, falling back to raw
  UTF-16BE comparison (formally verified monotonic for `Έδειχναν`, `Έριξε`,
  `Ακούμπησε`, `Ήταν`, `δείχνω`, `έδειχναν`, `χαρτοφύλακα(ς)`, `Χαρτοφύλακα`
  via a hand-written INDX/TAGX/IDXT decoder against kindling's encoder
  source).

- **On-device result: NEGATIVE.** Protocol followed correctly (full dictionary
  removal, power cycle, fresh install, distinct build title ruling out stale
  cache). Lowercase Greek still resolves. **Zero capitalized Greek words
  resolve — not just the four target words, but capitalized words in
  general.** This is *worse* than the original bug (specific words failing),
  which the original theory does not predict: a pure sort-order/routing fix
  should not turn a partial failure into total failure.

- **Investigation into device-side query normalization (this session):**
  Checked whether Kindle firmware lowercases the tapped query before search
  (which would mean no INDX/sort-order fix could ever work — the fix would
  need to be lowercase-alias generation instead). Findings:
  - `kindling-mobi-0.19.0`'s source contains **no simulation of on-device
    query normalization for the Latin/Greek path**. The only place kindling
    documents (from real hardware verification) that "the firmware encodes a
    tapped word the same way" it encodes labels is the **generated-ORDT path
    for Japanese/Chinese/Korean/Arabic** (`src/ordt.rs` module doc, README
    lines ~102). That mechanism is explicitly per-character and script-gated;
    it does not apply to Greek (`uses_generated_ordt("el") == false`).
  - For Greek/Latin, kindling embeds a **static, opaque blob**
    (`src/ordt_greek.bin`, extracted byte-for-byte from a real kindlegen
    Greek build) as the ORDT/SPL collation tables. kindling never decodes or
    interprets this blob's actual weights — it's just bytes copied in
    verbatim. The **only characterized behavior, per kindling's own README
    (line 100), is diacritic folding** ("looking up `meme` finds `même`").
    Case-folding is never claimed or coded anywhere for this blob.
  - **Directly verified against the real generated build** (`lemma_greek_en/content_*.html`)
    that all four target words already have *correct, matching* lowercase
    **and** capitalized iforms pointing at the right headword (e.g.
    `έδειχναν` and `Έδειχναν` both present as iforms of `δείχνω` in
    `content_08.html`; same confirmed for `ήταν`/`Ήταν`, `έριξε`/`Έριξε`,
    `ακούμπησε`/`Ακούμπησε`). This rules out an `entry_processor.rs`
    generation gap for these words — they are not indeclinables, so the
    capitalize/lower-first loop already runs and produces both forms.
  - **This is evidence *against* the query-lowercasing theory the on-device
    result might otherwise suggest.** If the firmware always lowercased the
    tapped query before search, a correctly-generated lowercase entry already
    exists for all four words, so they should resolve via that entry
    regardless of `strict_accents` — but they failed in *both* builds.

- **Revised, more parsimonious theory:** `capitalize_first()`
  (`src/entry_processor.rs:945`) uses Rust's `char::to_uppercase()`, which
  *preserves* the tonos when uppercasing a vowel (έ → Έ) — confirmed directly:
  the real build only ever contains the accented capital form (`Έδειχναν`),
  never an accent-stripped one (`Εδειχναν`). Greek typesetting frequently
  **drops the tonos on a capitalized first letter** (a long-standing printing
  convention, still common in books even under monotonic orthography). If the
  actual on-page word the user tapped was the unaccented capital
  (`Εδειχναν`), it was never indexed under **any** `strict_accents` setting —
  which explains the observed pattern exactly: total, identical failure in
  both builds (an input/vocabulary mismatch fails the same way regardless of
  collation), rather than the different-but-still-broken behavior a genuine
  MOBI-side collation bug would be expected to produce across two different
  ORDT configurations.

- **Recommended next step (not yet done — awaiting user decision):**
  1. Cheap, no-rebuild diagnostic: check the *exact* glyph of the tapped word
     on the actual page — does the capital letter carry a tonos mark or not?
     This distinguishes "input mismatch" (revised theory) from "still a real
     MOBI routing bug" (original theory not fully dead yet).
  2. If the capital is unaccented on the page: revert `strict_accents` to
     `false` (it demonstrated no benefit and regressed general capitalized
     lookup) and instead add generation of an accent-stripped capitalized
     iform variant alongside the existing accented one — a content-generation
     fix, not a MOBI-encoding one.
  3. If the capital on the page does carry the tonos: the routing-bug theory
     survives and needs a different device-side diagnostic (e.g. a minimal
     single-entry Greek test dictionary, A/B'd against a real kindlegen
     build, to isolate whether it's specifically the folding blob or
     something else in the INDX that's broken).

## Key technical facts worth preserving

- **kindling sorts Greek INDX labels by raw UTF-16BE byte value, not proper
  Greek collation.** This is the root mechanism behind the capitalization
  routing bug. Confirmed via `mobi.rs:2863-2865` in the vendored
  `kindling-mobi-0.19.0` crate (`~/.cargo/registry/.../kindling-mobi-0.19.0/`).
- **`exact="yes"` on `idx:iform`/`idx:orth` is written by lemma but never parsed
  by kindling** — confirmed the exact regex that extracts inflections
  (`kindling-mobi-0.19.0/src/opf.rs:641`, `r#"<idx:iform\s+value="([^"]*)""#`)
  captures only `value=`; neither `DictionaryEntry` nor `LookupTerm` has any
  field for it anywhere in the crate. Currently a complete no-op, for both
  `orth` and `iform` alike. Matching behavior is entirely governed by the
  device's own folding + the static/embedded Greek collation table (unless
  `strict_accents: true`).
- **Downloader cache**: real Kaikki data lives at `greek_data_en.jsonl` in the
  project root (the exact path `src/downloader.rs:75` checks first — a cache
  hit skips the network fetch entirely). `kaikki.org` is blocked by this
  environment's network policy; the real dataset was downloaded once from a
  GitHub release asset the user uploaded (`TDaly33/lemma` release
  `greek-data-en`) and has been reused for every real build since.
- **`--source en` is correct and is the default** — Greek headwords, English
  glosses (matches the README's "Modern Greek-English dictionary" framing).
  `--source el` would build Greek-Greek (Greek headwords, Greek glosses) from
  `elwiktionary` instead.
- Real production numbers at current shipped default (N=1, pre-`strict_accents`
  decision): ~31,467 headwords, content HTML ~118.6 MB, EPUB ~8.5 MB, MOBI
  ~50 MB / ~1.6M unique lookup terms.
