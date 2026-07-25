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

- **`strict_accents: true` in `src/mobi.rs`** (`08a575c`, `63a7298`) — committed
  as an explicit test flag, not yet a settled decision. See "Currently
  mid-decision" below; do not treat its presence in the tree as endorsement,
  it's there so the on-device test could happen at all.

Commits: `f4a609c`, `c9a50a6`, `13d90ad`, `345b060`, `08a575c`, `63a7298`.

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
  produced for the headword itself. Root cause confirmed directly against
  `EntryProcessor` output for `όχι`, `μα`, `έτσι`. **The proposed fix below
  (explicit capitalized-variant pass) would close this gap as a side effect**,
  since it scopes to the headword unconditionally rather than gating on the
  `forms` array.

- **ALL-CAPS word gap** — `entry_processor.rs` never generates full-uppercase
  variants (only first-letter-capitalized and all-lowercase), so any book text
  using full-caps styling (headings, emphasis) has no explicit iform to match.
  Previously this was framed as "an accepted tradeoff of `strict_accents: true`
  disabling device folding" — that framing needs revision now that the fold
  table is confirmed non-functional regardless of the flag (see below): this
  gap exists either way, `strict_accents` doesn't change it. Still unfixed,
  still out of scope for the current proposal (which only covers
  first-letter capitalization).

## `ordt_greek.bin` provenance investigation (concluded this session)

Traced the collation/folding table kindling embeds for Greek dictionaries to
understand whether it could ever be relied on, given the on-device
`strict_accents` test came back negative. Findings:

- **Not lemma's file.** Lives only inside the vendored `kindling-mobi-0.19.0`
  crate (`~/.cargo/registry/.../kindling-mobi-0.19.0/src/ordt_greek.bin`),
  embedded via `include_bytes!`. Nothing with this name exists in the lemma
  repo. lemma has no control over its contents.
- **Not Greek-exclusive despite the name.** Per kindling's own module doc:
  "Latin/Greek/Cyrillic dictionaries keep the UTF-16BE label scheme with the
  static Greek ORDT/SPL blob." One shared static blob covers three different
  script families under a single `OrdtMode::Greek` code path.
- **Decoded the actual bytes** (3906 bytes: `ORDT1`/`ORDT2`/`SPL1`-`SPL6`
  sections, offsets per kindling's own comments in `indx.rs`). Findings:
  - `ORDT1` (collation weights): **all zero bytes.**
  - `ORDT2` (codepoints): only 2 non-trivial u16 values (37, 95) — not
    plausible codepoints for any script, likely stray header data.
  - `SPL1`/`SPL2`/`SPL4`/`SPL5` (256-byte per-input-byte tables): almost
    entirely the `0xFFFF` unmapped sentinel.
  - `SPL6` (256-entry byte→u16 table): 229 of 256 entries unmapped. The
    *only* populated range is ASCII lowercase `a`-`z` (bytes `0x61`-`0x7A`),
    mapped to small arbitrary-looking numbers matching no real codepoint
    scheme. The entire `0x80`-`0xFF` byte range — where either a legacy
    single-byte Greek codepage (ISO-8859-7/Windows-1253) or a Latin-1/French
    one would live — is completely empty.
  - Conclusion: **not cleanly "a French/Latin-1 table misapplied to Greek"**
    (an earlier hypothesis) — closer to a near-empty placeholder populated
    with only generic ASCII-alphabet data, structurally incapable of
    encoding any accented script correctly. Can't confirm a specific French
    origin from the bytes alone.
- **Predates this fork entirely.** `kindling-mobi 0.19.0` was pinned in
  commit `18456ff`, well before this session's branch existed;
  `git diff --stat -- Cargo.toml Cargo.lock` is clean — nothing in this
  session touched the dependency. Any project building a Latin/Greek/Cyrillic
  dictionary against `kindling-mobi 0.19.0` — upstream open-greek/lemma
  included — embeds this same non-functional blob. Out of our control to fix
  upstream from here (`ciscoriordan/kindling` on GitHub is outside this
  session's repo scope; `TDaly33/lemma` is the only accessible repo).
- **Consequence:** there was never a working fold behavior to trade away by
  setting `strict_accents: true`. The ALL-CAPS gap noted above exists
  independent of this flag. The flag's only real effect is whether this
  (non-functional either way) table gets embedded in the primary INDX record
  at all — worth re-testing on its own merits once the real fix below is in
  place, not as a load-bearing part of the capitalization fix.

## Currently mid-decision

- **`strict_accents: true`** — on-device test was **negative** (total,
  identical failure of capitalized Greek lookups under both `true` and
  `false`, worse than the original partial-failure bug, ruling out the
  original "raw-byte routing mismatch" theory as the *complete* explanation).
  Leading theory now: `capitalize_first()` (`entry_processor.rs:945`, via
  Rust's `char::to_uppercase()`) *preserves* the tonos when uppercasing
  (έ→Έ), but Greek typesetting convention frequently *drops* the tonos on a
  capitalized initial letter. If the on-page glyph is the unaccented capital
  (Εδειχναν) and we only ever index the accented one (Έδειχναν), that's an
  input/vocabulary mismatch that fails identically regardless of MOBI-side
  collation settings — consistent with what was observed. Not yet confirmed
  against the actual on-page glyph (cheap, no-rebuild diagnostic still
  pending: does the tapped word's capital actually carry a tonos or not?).
  The `ordt_greek.bin` investigation above removes any reason to expect
  `strict_accents` to help either way, so this is now secondary to the
  proposal below rather than the primary fix path.

## Proposed next step: explicit capitalized iforms (design only, not yet implemented)

Goal: stop depending on kindling's device-side SPL/ORDT folding for
capitalization entirely — confirmed non-functional regardless of
`strict_accents` — by making every capitalized form an explicit,
independently-indexed iform, reusing the dictionary-wide collision-resolution
pattern already proven twice this session (Feature 1, Feature 2).

**What gets generated.** For every headword and every inflection that doesn't
already start with an uppercase letter, up to two explicit capitalized
variants:
1. *Accent-preserving capital* (current behavior, Rust's `to_uppercase()`):
   έδειχναν → Έδειχναν. Technically correct Unicode casing; keep it.
2. *Accent-dropped-on-first-letter capital* (new): έδειχναν → Εδειχναν.
   Matches the Greek typesetting convention that's the leading theory for
   the on-device failure. Only generated when the first letter actually
   carries a tonos (Ά/Έ/Ή/Ί/Ό/Ύ/Ώ → Α/Ε/Η/Ι/Ο/Υ/Ω) — for consonant-initial
   or already-unaccented words both variants would be identical, so skip
   the duplicate.

**Where it lives — two options, worth deciding explicitly:**
- *Option A (leaning this way):* new dictionary-wide pass in `html_gen.rs`
  (`build_capitalized_variants`, alongside `build_double_accent_variants` /
  `build_punct_variants`), scoped to headword + every inflection,
  unconditional on whether the word has a Wiktionary `forms` array — this is
  what closes the indeclinable-word gap (Όχι/Μα/Έτσι) as a side effect. Same
  collision resolution as the other two passes (dictionary-wide, drop on
  real-headword collision + log, frequency tiebreak on contested candidates).
  Migrates/retires the existing `capitalize_first`/`lower_first` calls in
  `entry_processor.rs` in favor of one consistent mechanism.
- *Option B:* keep `capitalize_first`/`lower_first` where they are, add the
  accent-drop variant there, and separately patch a headword-level step for
  the indeclinable gap. Smaller diff, but leaves two overlapping mechanisms
  instead of consolidating into one.

**Verification plan before committing to full implementation** (reusing the
INDX/TAGX/IDXT decoder already built this session):
1. Build a test `.mobi` with the new pass.
2. Re-run the decoder to confirm both new variants land in
   binary-search-consistent leaf positions (same check already done for the
   existing lowercase/capitalized forms).
3. On-device test specifically the previously-failing words. Which variant
   resolves (if either) is itself the decisive test of the accent-dropping
   theory: if the accent-dropped form resolves but the accent-preserving one
   still doesn't, that's confirmation. If *neither* resolves, something
   beyond string mismatch is still going on and the routing/`strict_accents`
   angle needs reopening rather than closing.

**Relationship to `strict_accents`:** independent decision. No working fold
behavior is being traded away by either setting (table confirmed
non-functional), so `strict_accents` can be decided on its own merits after
this fix lands, not as a prerequisite for it.

**Expected cost:** based on Feature 1 (0 real collisions / 234,585 variants)
and Feature 2 (13-16 real collisions per real build), a similarly low
collision rate is expected. Would need real-build measurement to confirm,
same as the other two features.

## Key technical facts worth preserving

- **kindling sorts Greek INDX labels by raw UTF-16BE byte value, not proper
  Greek collation.** Confirmed via `mobi.rs:2863-2865` in the vendored
  `kindling-mobi-0.19.0` crate. Formally verified (hand-written INDX decoder)
  that the routing table is nonetheless internally self-consistent under that
  same raw-byte comparison — so this alone does not explain the on-device
  failure; see the `ordt_greek.bin` section above for why the on-device
  behavior diverges from the formal structure.
- **`exact="yes"` on `idx:iform`/`idx:orth` is written by lemma but never
  parsed by kindling** — confirmed the exact regex that extracts inflections
  (`kindling-mobi-0.19.0/src/opf.rs:641`,
  `r#"<idx:iform\s+value="([^"]*)""#`) captures only `value=`; neither
  `DictionaryEntry` nor `LookupTerm` has any field for it anywhere in the
  crate. Complete no-op, for both `orth` and `iform` alike.
- **`ordt_greek.bin`** (see dedicated section above) is a near-empty
  placeholder shared across Latin/Greek/Cyrillic dictionaries, populated with
  only generic ASCII a-z data, structurally incapable of folding any accented
  script. Pre-existing upstream `kindling-mobi` issue, not lemma-specific,
  predates this fork, out of scope to fix from here.
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
- Real production numbers at current shipped default (N=1): ~31,467
  headwords, content HTML ~118.6 MB, EPUB ~8.5 MB, MOBI ~50 MB / ~1.6M unique
  lookup terms (pre-capitalization-fix baseline).
