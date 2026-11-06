# Changelog

## 0.7.0 (unreleased)

Two things a campaign of kept states needs from its writer. The replay specs of the BFCB ordered history were made
outside the writer, from pairs of kept trees; `replay make` makes them, through custodian_kit 0.5.1's
`change::replay_from_trees`, the rule that reads back through `change::replay_read` unchanged. And a runner could not
tell, before running a spec, whether the code-passenger on its PATH performs the spec's operation: 0.5.0 refuses
`operation = "replay"` only when the spec reaches it. `version` now says what it performs. On custodian_kit 0.5.1,
pinned at d930276. Minor, not patch: a new line, and a second line on `version`'s output that a reader of the first
line alone never sees. Links are pinned to c5c140c.

- **[`replay make`](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L608) `BEFORE AFTER --out DIR [--under P] [--name N] [--objective TEXT]`**, declared as every
  line is ([L52](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L52): `replay`, its action word `make` checked by [`replay_action`](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L104), as `dep` and
  `agent` take theirs; read at [L1174](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L1174)), run at [L1353](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L1353): a thin wrapper over the kit. It writes
  `change.toml` and `payload/` into DIR, which it makes; DIR already there is refused and nothing is written over.
  The spec is named N, else DIR's own name; its objective names both trees unless given. Anything either tree holds
  that a replay cannot carry refuses the whole spec, every path named: [`[CP0071]`](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/error.rs#L82), exit 2
  ([L114](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/error.rs#L114)), nothing written.
- **`version` says the operations it performs.** Its first line is the writer's name as before
  ([L1278 before](https://github.com/jameshuntt/code_passenger/blob/430eae7/src/lines.rs#L1278)); its second ([L1350](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L1350)) is `operations: ` and every word of the kit's
  `Operation` as compiled in ([`operations`](https://github.com/jameshuntt/code_passenger/blob/c5c140c/src/lines.rs#L418)): `transform` matches each one. custodian 0.8.0 reads it
  before a campaign runs and refuses a code-passenger that lists none, or not the one a spec uses.

Tests: `tests/replay.rs`: `replay_make_writes_the_spec_from_two_trees_and_transform_makes_the_after_tree` (a changed
file, a new one in a new folder, a deleted one, a mode alone; the spec named by its folder, its objective naming both
trees, DIR refused when there, then `transform --write` of the spec: the after tree, paths, bytes and modes),
`replay_make_refuses_what_a_replay_cannot_carry_naming_the_path_and_writes_nothing` (a link and an empty folder named,
`[CP0071]`, exit 2, no folder made; the line as read: no `--out`, another action, one tree, each refused);
`tests/lab_control.rs`: `the_version_line_names_the_writer_and_the_operations_it_performs` (two lines, the second
every word of `Operation`, `replay` among them).

`cargo test --test replay --test lab_control`: 31 passed, 0 failed.

## 0.6.0 (2026-11-03)

`replay`, custodian_kit 0.5.0's new operation, made by `transform`: an exact transition of whole files, of any type,
written only once the whole of it is checked, all of it or none, and put back by `--revert`. On custodian_kit 0.5.0,
pinned at its release commit (2a54db0). Minor, not patch: `Operation` gains a word, so 0.5.0 stays on custodian_kit
0.4.1 (built against a kit with the word, its exhaustive `match spec.op()` is `E0004`), and the kit's types in this
crate's API (`Outcome::record`, `lines::prior_accepted`) are 0.5.0's. Nothing is removed; no AST operation changes.
Links are pinned to f1a0b6f (eb48944 for CI). Reviews of the first cut (c694f44) found the write not all or none
after staging, no way to revert a replay, the left-out folders matched by exact case, the last checks made before the
checkpoint, and then (31c9fcf) the writer's own names taken over files already there: each is fixed below.

- **Its own path.** [`run`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2184) hands a replay spec to
  [`run_replay`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2043): nothing is scanned, parsed or compiled. The whole transition is read
  first by the kit's rule, the one the preflight keeps ([L1992](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2049), `change::replay_read`):
  every path placed by `change::replay_place` (not where the tree digest does not read, in any case: `.GIT`,
  `.Custodian`, `Target` too; not where its present part resolves outside the root or into one of them), every file
  as declared before (its sha256, or absent), every payload the bytes declared after, every mode; one that does not
  hold refuses the spec (`[CP0070]`, exit 2) and nothing is written, not even a checkpoint.
- **The governed write's checks, kept.** The preflight of this spec over this code (`authorize`),
  `--no-cargo`/`--unproven` writing nothing when governed, the tree unmoved since the plan. The proof is adapted: no
  cargo runs (a replay reproduces a kept state; the experiment's verification judges it); the transition is made on a
  copy of the files it names and each after state, bytes and mode, read back there
  ([L2013](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2070)). With `--write`, the checkpoint is taken first
  ([L2075](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2132)); then, just before placing, the tree is checked unmoved
  ([L2079](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2136)) and the transition read again, which must be the one proved, file for file and
  byte for byte (in place of `as_proved`).
- **The write, all or none.** [`make_replay`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1910) does everything that can fail beside the
  files first: the folders a new file needs, each payload staged with its bits ([`set_mode`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1833)),
  a copy kept of each file to be replaced and of each file to be deleted (a hard link, else a copy created new:
  [`copy_new`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1888)), the deleted file's name then taken away; then each payload is put in
  place ([L1954](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1954)). Any failure undoes every step done, the last first
  ([L1960](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1960)), and says what failed and that every file is as it was (or, if one could not be
  put back, which); only then are the writer's own files taken away. Every file the writer makes for itself is made
  under a name nothing had ([`own_beside`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1872): `.<name>.replay-<pid>`, then `-1` ... `-15`,
  each created exclusively, else refused while staging), so a file already at one of those names is never written
  over or taken away; 31c9fcf fell back to a copy over an existing name and renamed over one. In c694f44 the deletes
  and placements ran one by one after staging. Each after state is read back in the tree
  ([`replay_after`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2029)); a file of the writer's own it could not take away is a postcondition
  that does not hold ([`left_fact`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2010), [L2149](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2149)), so the record
  says it and the line exits 1.
- **A replay can be reverted.** Before placing, each file it replaces or deletes has its bytes kept in the passenger's
  objects ([`keep_before`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L2016), through the store's one [`keep`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/passenger/store.rs#L442))
  and, beside the write's checkpoint, its bits and the folders the write made ([`ReplayKept`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L1998)); a checkpoint keeps
  only cargo's files, so c694f44's replay could not be put back. `--revert` of a replay's record
  ([L3089](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L3159), [`revert_replay`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L3022)) is the replay made backwards by
  the same all-or-none write and the same placing rule, while every file is exactly as the replay left it: files it
  made taken away, files it replaced or deleted written again with their bytes and bits, and the folders the replay
  made taken away when empty again ([L3067](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L3067)); any other folder stays. No cargo runs. An
  object is named only by a sha256 (64 lowercase hex): a record with any other digest is refused, never a panic
  ([`object_path`](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/passenger/store.rs#L419)).
- **The record** names exactly the files the replay names, in path order, each digest before and after (`none`:
  absent), the lines written where both sides are text (so a job's close covers them), and one postcondition per file
  (`replay <path>: sha256 <after>, mode <bits>`, or `absent`); `operation` `replay`, `target` `*`.
- **A batch leaves a replay out** and says why ([L653](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L653)): a replay is made alone.
- **CI** ([`package`](https://github.com/jameshuntt/code_passenger/blob/eb48944/.github/workflows/ci.yml#L55)): the crate as crates.io will hold it, built on every push
  against crates.io's custodian_kit; while that version of custodian_kit is not published yet (a release made with
  the kit's) it says so and passes, and `cargo publish` builds the same .crate once the kit is there. Package and
  publish wait only on custodian_kit's version missing, never on another resolution error.

Tests: `tests/replay.rs`: `one_file_is_replaced_byte_for_byte_and_the_record_names_it`,
`a_mixed_transition_makes_replaces_and_deletes_with_its_modes_and_the_record_names_exactly_those_files` (text,
bytes, a made executable, 0640, a replaced file's own 0600, a delete), `a_dry_run_proves_the_transition_and_changes_nothing`,
`each_way_a_transition_does_not_hold_is_refused_before_anything_is_written` (14 refusals, each with and without
`--write`; `.GIT/hooks/post-checkout`, `.Custodian/state/j.txt`, `Target/x` among them),
`a_five_file_replay_whose_fifth_file_is_not_as_declared_writes_none_of_them`,
`a_late_payload_that_is_not_the_bytes_declared_writes_nothing`, `a_staging_that_fails_takes_back_what_it_staged`,
`a_delete_in_a_folder_that_cannot_be_written_writes_nothing_and_says_so`,
`a_replay_is_reverted_to_the_exact_state_before` (a set-user-id file kept through the write and the revert, the
folder it made taken away), `files_at_the_writers_own_names_are_never_written_over_or_taken_away` (files planted at
the staging, kept and deleted-copy names: untouched; all sixteen names taken: refused, nothing changed),
`a_record_with_a_digest_that_is_not_a_sha256_is_refused`,
`in_a_governed_project_a_replay_is_written_only_on_the_preflight_of_this_spec_over_this_code`,
`a_batch_leaves_a_replay_out_and_says_why`, `the_transform_line_runs_a_replay_spec_written_by_hand`;
`transform::tests::a_placing_that_fails_puts_every_file_back` (planted names too) ([L3307](https://github.com/jameshuntt/code_passenger/blob/f1a0b6f/src/transform.rs#L3377));
`every_operation_of_the_vocabulary_has_its_test_here` reads `tests/replay.rs` too.

`cargo test --all-features`: 117 passed, 0 failed (stable and Rust 1.90); 14 of them in `tests/replay.rs`, 1 in
`transform::tests`.

## 0.5.0 (2026-10-30)

The laboratory-control repair (2026-10-30), on custodian_kit 0.4.1 (0.4.0's repair; 0.4.1 counts a write made with
`code-passenger transform --preflight N --write` as the open experiment's change). Round 49's dry run compiled under
twelve configurations, passed its tests, showed `let mut matching_index = None;` in its diff, and reported that line
absent; and `transform --write` wrote whatever `--preflight N` said, unchecked. This release proves a change on its
copy only, writes it only on the preflight that covered it, and writes only the tree it proved. Minor, not patch: a
governed write now needs its preflight, and lines that wrote freely in a governed project refuse while an experiment
is open. Nothing is removed. Links are pinned to 6907c7e; the lines they replace to 23c6a46 (0.4.9).

- **A fact is read on the proof's copy only.** `holds` joined a fact's path onto the copy's root
  ([L1650](https://github.com/jameshuntt/code_passenger/blob/23c6a46/src/transform.rs#L1650)); an absolute path (the specimen's, which a root spelled `.` made) replaced it,
  so the fact read the specimen. [`inside`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1623) takes a path relative to the copy and inside
  it, else the fact does not hold. Tests: `replace_bodys_text_fact_is_proved_on_the_proofs_copy_with_the_root_spelled_dot`,
  `a_fact_naming_a_file_outside_the_proof_copy_never_holds`.
- **A write goes only on the preflight that covered it.** In a project custodian keeps (found from the root as given,
  from each file the write touches, then the workspace's: [`governing`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1730)),
  [`authorize`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1716) asks custodian_kit's `preflight::authorizing` for this exact spec over this
  exact code, before the proof; the record's `preflight_seq` is the receipt it went on, not the number given
  ([L1857](https://github.com/jameshuntt/code_passenger/blob/23c6a46/src/transform.rs#L1857)). A named receipt is checked on a dry run too. Outside a governed project nothing
  is asked, and `--preflight` is refused. `--no-cargo`/`--unproven` write nothing when governed
  ([`proved_writes_only`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1754)). The batch asks the same for each member, and refuses two members
  of one name. Tests: `an_authorized_exact_spec_over_exact_code_is_written_and_bound_to_its_preflight`,
  `an_altered_spec_with_the_old_receipt_is_refused_and_nothing_is_written`, `altered_code_with_the_old_receipt_is_refused_and_nothing_is_written`,
  `a_governed_write_needs_a_preflight_and_a_dry_run_does_not`, `a_governed_write_is_a_proved_one`,
  `a_member_crates_own_custodian_folder_governs_it`, `a_batch_of_two_specs_of_one_name_is_refused`,
  `a_governed_write_from_root_dot_goes_on_its_preflight`.
- **The tree written is the tree proved.** Before a write, [`unmoved`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1765): the code digest is
  the one read before the plan, every file of the tree is as it was then (none added, gone or changed), and each edited
  file holds its planned bytes; [`as_proved`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1791): each written file is the proof copy's, the
  lock is the one the proof built on, and every other file of the tree (what the code digest leaves out too: a build
  script, a file `include_str!` reads) is the copy's. The edits were written with no such check
  ([L1884](https://github.com/jameshuntt/code_passenger/blob/23c6a46/src/transform.rs#L1884), [L2395](https://github.com/jameshuntt/code_passenger/blob/23c6a46/src/transform.rs#L2395)). The record keeps `code_digest` (before the
  plan), `code_digest_after`, `lock`, `writer`. Test: `a_tree_that_moved_or_a_copy_that_differs_writes_nothing`.
- **A manifest changed as a side effect carries the proof's lock.** A toolkit piece's dependency, or one removed, changed
  `Cargo.toml` without the lock cargo resolved for it in the proof ([L1917](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1917)); single and
  batch. `replace_body_keeps_the_signature_and_docs_and_add_test_lands_in_the_module_named` now lists `Cargo.lock` among
  the files written.
- **A revert puts back only a recorded transform inside the tree.** A record's paths were joined onto the root unchecked,
  so an absolute one reached outside it; now each must be inside it. Governed, each record must be named exactly by a
  receipt in the journal ([L2694](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L2694)) and be byte for byte the copy code_passenger keeps with its
  write's checkpoint ([L2702](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L2702), [`keep_record`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/transform.rs#L1747)). Tests:
  `a_revert_puts_back_only_a_recorded_transform_inside_the_tree`, `a_revert_never_reaches_outside_the_tree`.
- **An open experiment's tree is written only by an authorized transform.** `dep add`, `attr`, `gate --write` refuse while
  a running job holds a provenance record, found from the root or from any file they would write
  ([`outside_an_experiment`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/gate.rs#L98)), and write as before otherwise. Test:
  `an_open_experiments_tree_is_written_only_by_an_authorized_transform`.
- **The knowledge graph's item `target`.** Each workspace item record carries the change spec's target as it is
  ([`with_targets`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/commands/receipt.rs#L19)). Test: `the_knowledge_graphs_target_is_a_change_specs_target_as_it_is`.
- **`version`** (and `--version`): `code_passenger 0.5.0 (custodian_kit 0.4.1)` ([`writer`](https://github.com/jameshuntt/code_passenger/blob/6907c7e/src/lines.rs#L405)).
  Test: `the_version_line_names_the_writer`.
- **The siblings pinned.** `siblings.toml` names the commit of custodian_kit this tree is tested against;
  `scripts/siblings.sh` puts it at `../custodian_kit` (an existing checkout is checked, never changed). CI runs it in
  place of cloning the sibling's branch head.

`cargo test --all-features`: 102 passed, 0 failed. `tests/lab_control.rs`: 15.

## 0.4.9 (2026-10-23)

The whole-workspace swap job ran its batches on 282 crates and found where the batch's proof costs too much or gives
up: every configuration of every package compiled for a swap that touches no cfg (the proof folder grew to 168 GB),
the first failing package ended the round, eight rounds and the batch stopped without saying why, a configuration that
already failed on the base failed the whole batch, a package named `recipe-session-transfer-plan-adapter` was asked
for as `recipe_session_transfer_plan_adapter`, and an optional dependency's implicit feature was passed to cargo,
which refuses it. This release fixes each. Nothing is removed from the API. Links are pinned to e664bc8; the lines
they replace to c7a77ae (0.4.8). custodian_kit 0.3.14.

- **One configuration per package, the matrix only for a cfg change.** A package is checked in its default
  configuration; klepto's matrix only for a package a member changes the manifest of or writes a `cfg(` in
  (`touches_cfg`, [transform.rs L2113](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/transform.rs#L2113); `configurations_of`,
  [L2120](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/transform.rs#L2120)). Every package ran the whole matrix
  ([transform.rs L2166](https://github.com/jameshuntt/code_passenger/blob/c7a77ae9e64f5285435f4230d27cd71e84580a5d/src/transform.rs#L2166)).
- **Never an optional dependency's implicit feature.** `configurations_of` leaves out a feature cargo makes for an
  optional dependency, and a configuration is checked once. Same lines.
- **A failure the base has is not the batch's.** A configuration that fails is checked again on the base with the
  batch's edits left out, once per configuration ([transform.rs L2174](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/transform.rs#L2174)); one that fails
  the same there is said per package and left out of the proof ([L2224](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/transform.rs#L2224)), and a member
  whose own package compiles in no configuration on the base is left out and named. It failed the batch.
- **Every package checked, every round, until the members left compile.** The rounds were eight
  ([transform.rs L2144](https://github.com/jameshuntt/code_passenger/blob/c7a77ae9e64f5285435f4230d27cd71e84580a5d/src/transform.rs#L2144)) and each stopped at the first failing package
  ([L2172](https://github.com/jameshuntt/code_passenger/blob/c7a77ae9e64f5285435f4230d27cd71e84580a5d/src/transform.rs#L2172)). Now each round checks every package, leaves out every member an error points
  into, and runs until none is left out; errors no member's lines hold end the batch, naming the packages
  ([L2261](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/transform.rs#L2261)).
- **Packages as cargo names them.** `-p` takes the manifest's `[package] name` (`manifest_name`,
  [cargo.rs L74](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/cargo.rs#L74); `ProofDir::cargo_name`, [proof_dir.rs L70](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/proof_dir.rs#L70)) in the
  check ([oracle.rs L151](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/oracle.rs#L151)), the expansion and the tests.
- **The kept build's incremental folders go after each batch** ([transform.rs L2437](https://github.com/jameshuntt/code_passenger/blob/e664bc87c6034b62be6116baf79257ad172c0f56/src/transform.rs#L2437)),
  and custodian_kit 0.3.14 holds the build under its cap before each compile.

Test: `a_batch_checks_the_default_configuration_names_packages_as_cargo_does_and_goes_past_a_base_failure`
(tests/batch.rs): a member is checked in the default configuration only, the dependent renamed `rule-user` is
checked as cargo names it, then broken on the base the batch goes past it, says so, writes its member, and the
incremental folders are gone. `cargo test`: 85 passed, 0 failed.

API, additive: `cargo::manifest_name`, `cargo::cargo_names`, `ProofDir::cargo_name`, `ProofDir::prune_incremental`.

## 0.4.8 (2026-10-23)

On custodian_kit 0.3.11. Two agents' runs on the kit found the writer's gaps: undoing a swap cost 77 s each and
could not undo a batch in one step, two `From` impls on one type had one target, a dependency change could not join
a batch, a failed proof check gave no reason, the proof's copy could miss the workspace root cargo looks for, and a
merge had to drop a dependency and move a crate's paths by hand. Each is fixed here with a test on a fixture.
Nothing is removed from the API (the rustdoc JSON of 0.4.7 and 0.4.8 differ by 2 additions and no removal). Links are pinned to dac7b45; the lines they replace to 3142101 (0.4.7).

- **Revert.** `revert` ([transform.rs L2402](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L2402)) takes a batch's folder and puts each file back to
  before its first member in one step (`chained`, [L2472](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L2472)); it snapshots the files instead
  of scanning the tree twice, and pays one `cargo check` on the kept build (`check_once`, [L2492](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L2492));
  its record holds each file's digest now, so a close covers it. Was [transform.rs L2340](https://github.com/jameshuntt/code_passenger/blob/3142101f3217404218fbcf56f87697258ce10027/src/transform.rs#L2340).
  Test: `a_batchs_members_chain_within_a_file_so_the_batch_reverts_to_before_its_first`.
- **Routes with their impl.** Targets resolve through custodian_kit's `route::find_fn`; an expansion finds a function in
  the impl the route names (`in_named_impl`, [to_toolkit.rs L199](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/to_toolkit.rs#L199)).
- **Manifest changes in a batch.** A dependency change is a member like a code change: one scan, one check, cargo's
  lock carried with the write (`batch_lock`, [transform.rs L2100](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L2100)); it was left out
  ([transform.rs L2035](https://github.com/jameshuntt/code_passenger/blob/3142101f3217404218fbcf56f87697258ce10027/src/transform.rs#L2035)). Test: `a_batch_of_dependency_changes_is_one_transform_for_every_caller`.
- **remove_dependency and rename_paths.** [transform.rs L1118](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L1118) drops a dependency once no source
  file of the crate names it (`crate_names`, [L2566](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L2566), each place said); [L1134](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L1134)
  moves every `old::` path to `new::` (`rename_crate_paths`, [L2589](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L2589)); both proved by the compile.
  Tests: `rename_paths_then_remove_dependency_with_the_use_site_check`, `rename_paths_moves_every_path_through_one_crate_and_nothing_else`.
- **A failed check says why.** `first_error_line` ([oracle.rs L213](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/oracle.rs#L213)) carries cargo's first error line
  where it took its last ([oracle.rs L188](https://github.com/jameshuntt/code_passenger/blob/3142101f3217404218fbcf56f87697258ce10027/src/oracle.rs#L188)), and "nothing written" names it
  ([transform.rs L1880](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/transform.rs#L1880)). Test: `a_failed_check_with_no_compiler_message_carries_cargos_first_error_line`.
- **The workspace root.** When cargo cannot load the workspace, the root is the governing root by the requirements
  reading's rule ([gate.rs L304](https://github.com/jameshuntt/code_passenger/blob/dac7b4531dcf51d8c022ebea50f0f7514eb1f104/src/gate.rs#L304)), so the proof's copy carries it.

## 0.4.7 (2026-10-20)

On custodian_kit 0.3.9. A test run after last night's fleet_ops swaps built every dependent's dependent from an empty
build in the worktree, for six hours, on battery: the proof's kept build sat in code_passenger's place where nothing
else could use it. The place is the kit's now, so custodian's test runs use the same copy and build. And James's rule
for a laptop: no cargo line starts on a dying battery. Nothing is removed from the API: the rustdoc JSON of 0.4.6 and
0.4.7 is the same, item for item. Every link is pinned to f55f12e; the lines it replaces to dcaae55 (0.4.6).

- **The proof's place is custodian_kit's.** `proof_dir::ProofDir` holds a `custodian_kit::proof_place::ProofPlace`
  ([proof_dir.rs L64](https://github.com/jameshuntt/code_passenger/blob/f55f12e/src/proof_dir.rs#L64), [proof_place.rs L80](https://github.com/1o-llc/custodian_kit/blob/2899848/src/proof_place.rs#L80)): the same path
  rule, lock, sync and no-debug-info environment as 0.4.6 kept in proof_dir.rs itself ([proof_dir.rs L88 at 0.4.6](https://github.com/jameshuntt/code_passenger/blob/dcaae55/src/proof_dir.rs#L88)),
  one copy of them, so the proof and custodian's `test run` share one kept build per workspace. Proof: tests/batch.rs
  `the_proof_place_is_kept_and_a_sync_writes_only_what_differs`, as it was.
- **No cargo line on a dying battery.** Under 20% and not charging (`pmset -g batt`), the proof's place is not taken
  and no cargo line starts: `[CUST0042]`, nothing written ([proof_place.rs L80](https://github.com/1o-llc/custodian_kit/blob/2899848/src/proof_place.rs#L80), the
  checks code_passenger runs itself, [oracle.rs L166](https://github.com/jameshuntt/code_passenger/blob/f55f12e/src/oracle.rs#L166), and its `cargo locate-project`,
  [gate.rs L295](https://github.com/jameshuntt/code_passenger/blob/f55f12e/src/gate.rs#L295)). Proof: tests/battery.rs
  `a_dying_battery_refuses_the_proof_before_any_cargo_line_and_writes_nothing` ([L10](https://github.com/jameshuntt/code_passenger/blob/f55f12e/tests/battery.rs#L10)),
  the battery given by `CUSTODIAN_BATTERY`.

`cargo test --locked --all-features` passes; `cargo clippy --locked --all-targets --all-features -- -D warnings` is clean.

## 0.4.6 (2026-10-20)

On custodian_kit 0.3.8: the proof's new shape, and James's rule for a repeated shape. A body swapped in the kit's
fleet_ops with 0.4.5 was proved on a fresh copy of the workspace with an empty build, compiling every crate that
depends on fleet_ops, near and far: one swap took 19.4 minutes and 37 GB, and the next swap paid it all again. Its
`to-toolkit` refused every fleet_ops swap, because 23 of the crate's tests fail on a Mac before any change. Measured
on the same swap (`fleet_ops::survey::main_pid`, a `return_if`) with 0.4.6: 6.7 minutes and a 20 GB build the first
time, kept; 2.7 minutes and about 1 GB more for the next swap; 8 packages and 12 configurations checked, the
expansion the same, the 23 failures named as the base's. Nothing is removed from the API: the rustdoc JSON of 0.4.5
and 0.4.6 differ by 132 additions and no removal. Every link is pinned to b628f28; the lines it replaces to 70f0929 (0.4.5).

- **One proof place per workspace.** `proof_dir::ProofDir` ([proof_dir.rs L88](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/proof_dir.rs#L88)): a copy of the
  workspace at a path that does not change and one build folder kept beside it, under
  `<temp>/code_passenger-proof/<name>-<digest of its path>` (or `$CODE_PASSENGER_PROOF_DIR`). A proof syncs the copy by
  bytes ([proof_dir.rs L121](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/proof_dir.rs#L121)): a file whose bytes differ is written, a file the workspace no
  longer has goes, nothing else is touched, so cargo compiles again only what moved. One proof holds it at a time (a
  lock file). Every cargo line of a proof runs on the kept build with no debug info (`PROOF_ENV`,
  [proof_dir.rs L51](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/proof_dir.rs#L51)). 0.4.5 made a fresh copy and an empty build per proof
  ([oracle.rs L87 at 0.4.5](https://github.com/jameshuntt/code_passenger/blob/70f0929/src/oracle.rs#L87), [oracle.rs L119 at 0.4.5](https://github.com/jameshuntt/code_passenger/blob/70f0929/src/oracle.rs#L119)). The place is
  never inside the workspace's `target/`: klepto reads no source under a `target` folder, so the proof's scan found
  nothing there. A place whose workspace is gone is taken away when another is taken ([proof_dir.rs L66](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/proof_dir.rs#L66)).
  Proof: tests/batch.rs `the_proof_place_is_kept_and_a_sync_writes_only_what_differs` ([L72](https://github.com/jameshuntt/code_passenger/blob/b628f28/tests/batch.rs#L72)).
- **The crate and its direct dependents.** The transform checks the packages custodian_kit's `change::proof_packages`
  names ([transform.rs L560](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L560)), its tests compiled only in the crate it lands in
  ([oracle.rs L130](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/oracle.rs#L130)); 0.4.5 walked the closure of dependents
  ([transform.rs L1651 at 0.4.5](https://github.com/jameshuntt/code_passenger/blob/70f0929/src/transform.rs#L1651)).
- **The expansion is the transform's postcondition.** For a spec with `same_expansion`, `cargo expand` of the function
  before (the copy as the tree is) and after (the edits written) on the kept build ([transform.rs L1701](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L1701)),
  compared in custodian_kit's proof form, is the postcondition `expands <function> the same`
  ([transform.rs L1772](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L1772)), its two expansions kept beside the record
  (`<record>.expansion.json`). 0.4.5 checked it only inside `to-toolkit`, which no custodian receipt carried.
- **The tests against the base.** `transform --tests` runs the crate's tests on the proof and, when some fail, the same
  tests with the change taken out (`ProofDir::tests_compared`, [proof_dir.rs L207](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/proof_dir.rs#L207)): a test
  failing the same before is not the change's; one the change breaks stops it (`tests <crate> as before`). `to-toolkit`
  is now that transform ([to_toolkit.rs L253](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/to_toolkit.rs#L253)); 0.4.5 asked for exit 0
  ([to_toolkit.rs L257 at 0.4.5](https://github.com/jameshuntt/code_passenger/blob/70f0929/src/to_toolkit.rs#L257)). A dry `to-toolkit` on a crate that does not depend on
  simple_impl yet now says it needs `--write` for the dependency step first.
- **James's rule for a repeated shape.** Prove one member of a shape fully, then batch the rest of that shape in the
  crate under one compile, one receipt per member, a failing member excluded and named. It holds for every operation
  with repeated shapes (a gate, a derive, a rename), not only a swap. `transform --batch LIST [--after-proof N]
  [--tests]` (`transform::run_batch`, [transform.rs L2062](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L2062)): the members planned over one
  scan ([transform.rs L2027](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L2027)), a member whose edit overlaps another's or that changes a
  manifest left out, the rest placed together ([transform.rs L1976](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L1976)), one cargo check over
  the check matrix, one expansion per crate before and after, one scan for every postcondition, the crates' tests once;
  a member an error points into, whose postcondition does not hold or whose expansion is not the same is left out and
  named and the rest checked again; one checkpoint before and after (`a batch of N after proof #S`), one record per
  member. `--toolkit` takes functions by path and finds each again with the matcher first
  ([to_toolkit.rs L385](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/to_toolkit.rs#L385)).
- **A member reverts alone.** `transform --revert` of a member written before others takes out only its own change
  when the file moved since, found once as it wrote it (`take_out`, [transform.rs L2384](https://github.com/jameshuntt/code_passenger/blob/b628f28/src/transform.rs#L2384));
  0.4.5 refused any file that moved.

Proof: tests/batch.rs ([L38](https://github.com/jameshuntt/code_passenger/blob/b628f28/tests/batch.rs#L38), [L95](https://github.com/jameshuntt/code_passenger/blob/b628f28/tests/batch.rs#L95), [L130](https://github.com/jameshuntt/code_passenger/blob/b628f28/tests/batch.rs#L130)) on
tests/fixtures/batch_ws (rules, user depending on it, far depending on user): one swap checks rules and user, never
far, its expansion the same and the failing-before test named as the base's; a swap not the same tokens and a change
that breaks a passing test are refused; a batch of three after a proof leaves out the one not the same and writes the
two, one record and one expansion each, and one of them reverts alone; the `--toolkit` line swaps the shape and leaves
out a function not in the code. `cargo test --all-features` and `cargo clippy --all-targets --all-features -- -D
warnings` pass.

## 0.4.5 (2026-10-20)

On custodian_kit 0.3.7: the writer's half of three findings custodian's exercise made on the pantry with 0.4.4.
Nothing is removed from the API: the rustdoc JSON of 0.4.4 and 0.4.5 has the same 2043 items. Every link is pinned to
4456b83; the lines it replaces to 4fe7938 (0.4.4's tree).

- **Calls inside a macro's arguments get the new argument.** `change_signature` gave it only to the calls klepto
  resolves ([transform.rs L1069 at 0.4.4](https://github.com/jameshuntt/code_passenger/blob/4fe7938/src/transform.rs#L1069)); the pantry's two calls of `shopping_list` inside
  `simple_impl::ensure_eq!` and `ensure_empty!`, which klepto names in the tokens and resolves none, were left, and
  the proof stopped the change. The calls are now custodian_kit's `change::macro_calls`, the rule the preflight counts
  by ([transform.rs L1119](https://github.com/jameshuntt/code_passenger/blob/4456b83/src/transform.rs#L1119)), each found from the macro to the bracket closing its arguments, over as many
  lines as it takes, by the path its tokens write (`list(..)`, `crate::list(..)`), never after `.` or another
  segment ([transform.rs L504](https://github.com/jameshuntt/code_passenger/blob/4456b83/src/transform.rs#L504)). Proof: tests/transform_more.rs
  `a_call_inside_a_macro_klepto_cannot_expand_gets_the_new_argument` ([L211](https://github.com/jameshuntt/code_passenger/blob/4456b83/tests/transform_more.rs#L211)): two
  calls inside a function-like proc macro, one over three lines, get the argument and the tests compile.
- **A dry run that does not compile says so.** It said "proved on a scratch copy, not written" either way
  ([transform.rs L1744 at 0.4.4](https://github.com/jameshuntt/code_passenger/blob/4fe7938/src/transform.rs#L1744)). It now says `not proved:` with the first configuration that does
  not compile, the first postcondition that does not hold, or that nothing was checked ([transform.rs L1779](https://github.com/jameshuntt/code_passenger/blob/4456b83/src/transform.rs#L1779),
  [transform.rs L1793](https://github.com/jameshuntt/code_passenger/blob/4456b83/src/transform.rs#L1793)).
- **The fix for code naming an optional dependency.** A proof that fails for code naming an optional dependency with
  no feature gate says the spec's fix, `attrs = ['#[cfg(feature = "..")]']`, from custodian_kit's
  `change::optional_uses`, the rule the preflight refuses by ([transform.rs L1709](https://github.com/jameshuntt/code_passenger/blob/4456b83/src/transform.rs#L1709)). Proof:
  `a_dry_run_that_does_not_compile_says_not_proved_and_names_the_attrs_fix` ([L226](https://github.com/jameshuntt/code_passenger/blob/4456b83/tests/transform_more.rs#L226)):
  `b::b()` in p with no gate is not proved, the line says why and the fix; with the attribute it is proved.
- The feature model a change leaves is custodian_kit's `change::model_after` ([transform.rs L1657](https://github.com/jameshuntt/code_passenger/blob/4456b83/src/transform.rs#L1657)), the
  rule the preflight now counts configurations by; 0.4.4 had its own ([transform.rs L1532 at 0.4.4](https://github.com/jameshuntt/code_passenger/blob/4fe7938/src/transform.rs#L1532)).

`cargo test --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` pass.

## 0.4.4 (2026-10-20)

The writer for what custodian_kit 0.3.6 adds after the pantry: features written to a manifest, a dependency made
optional, a library beside a binary, a signature and its body in one step, a test module made where one is asked
for. Also the `to-toolkit` line from the `patterns` branch. Nothing is removed from the API: the rustdoc JSON of
0.4.3 and 0.4.4 differ by 90 additions and no removal. Every link is pinned to e852921.

- **One place for a manifest's features and `optional`.** `edit::set_optional_in`, `edit::set_feature_in` and
  `edit::feature_declare_text` ([edit.rs L336](https://github.com/jameshuntt/code_passenger/blob/e852921/src/commands/edit.rs#L336), [edit.rs L371](https://github.com/jameshuntt/code_passenger/blob/e852921/src/commands/edit.rs#L371),
  [edit.rs L386](https://github.com/jameshuntt/code_passenger/blob/e852921/src/commands/edit.rs#L386)); the features plan's manifest edits use them too ([gate.rs L269](https://github.com/jameshuntt/code_passenger/blob/e852921/src/gate.rs#L269)), where
  0.4.3 had its own ([gate.rs L271 at 0.4.3](https://github.com/jameshuntt/code_passenger/blob/dc8e2db/src/gate.rs#L271)). `edit::dep_change_text_with` takes `optional`, alone (the
  source kept) or with a new source ([edit.rs L297](https://github.com/jameshuntt/code_passenger/blob/e852921/src/commands/edit.rs#L297)).
- **Cargo's spacing.** A key added to an inline entry is written as cargo writes it: `features = ["derive"],
  optional = true`, where 0.4.3 wrote `["derive"] , optional = true` (`edit::tidy_entry`,
  [edit.rs L100](https://github.com/jameshuntt/code_passenger/blob/e852921/src/commands/edit.rs#L100)).
- **declare_feature** ([transform.rs L1017](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L1017)): the feature written, and the required dependencies its
  `dep:` entries name made optional in the same step; the postconditions `feature <crate> <name> <list>` and
  `optional <crate> <dependency> <bool>` ([transform.rs L1499-L1513](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L1499-L1513)). The check matrix is read from the
  manifest as the change leaves it ([transform.rs L1532](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L1532)), where 0.4.3 read the model from before the edit
  ([transform.rs L1520 at 0.4.3](https://github.com/jameshuntt/code_passenger/blob/dc8e2db/src/transform.rs#L1520)).
- **The lock file with the manifest.** A transform that changes a manifest writes the lock file cargo made in its
  proof ([transform.rs L1647](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L1647)): the tree's lock names what its manifests name, so a frozen scan reads the
  new dependency's macros and sees the functions its generators write (the pantry's `can_cook`).
- **make_lib** ([transform.rs L774](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L774)), **change_signature with a body** ([transform.rs L1046](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L1046)),
  **add_test makes its module** ([transform.rs L816](https://github.com/jameshuntt/code_passenger/blob/e852921/src/transform.rs#L816), where 0.4.3 refused, [transform.rs L800 at 0.4.3](https://github.com/jameshuntt/code_passenger/blob/dc8e2db/src/transform.rs#L800)).
- **to-toolkit** (from `patterns`): one function's body swapped for the toolkit calls whose expansion it already
  is, each step a change spec, proved on a scratch copy by `cargo expand` of the function before and after and the
  crate's tests; a near match refused with its difference named.

Proof, in [tests/manifest_ops.rs](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs) on `transform_more` and the new fixture `feature_forms` (p depends on a, a on
c, b optional; a declares `x = ["c/z"]`):
- `declare_feature_writes_every_cargo_form_and_its_record_covers_the_lines_written` ([L129](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L129)): `g`,
  `a/x`, `b?/y` and `dep:b` in one feature, the record covering the one line written, every configuration of the
  new matrix compiled; `a/y` refused with a's features named.
- `a_feature_named_like_the_dependency_it_turns_on_is_written` ([L47](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L47)),
  `a_feature_is_declared_and_its_dependency_made_optional_in_one_step` ([L33](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L33)),
  `optional_on_add_dependency_and_change_dependency` ([L61](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L61)): the lock written, and a frozen
  `cargo metadata` reads it; `optional_keeps_a_dependencys_features_and_cargos_spacing` ([L149](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L149)),
  `a_key_added_to_an_inline_entry_keeps_cargos_spacing` ([L80](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L80)).
- `make_lib_gives_a_binary_only_crate_its_library` ([L90](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L90)),
  `change_signature_with_a_body_writes_both_in_one_step` ([L103](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L103)),
  `add_test_makes_the_module_it_is_asked_to_go_in` ([L114](https://github.com/jameshuntt/code_passenger/blob/e852921/tests/manifest_ops.rs#L114)).
- `cargo test --all-features` and `cargo clippy --all-targets --all-features -- -D warnings` pass.

## 0.4.3 (2026-10-20)

custodian's exercise ran every operation on copies of three projects and found six faults in `transform`; the first
bounded job in the kit found two more. Each is fixed here, with `inline` and `wrap`, the two operations the
vocabulary did not have. On custodian_kit 0.3.3. Nothing public removed or changed: the rustdoc JSON of 0.4.2 and
0.4.3 differ only by added items (the check line's `--lib`, `--bins` and `--tests`, the oracle's `tests`). New code
is pinned to 49d4d5b.

### A new module's folder

- Was: `add_module` and `extract` into a module that has its own file (`src/names.rs`) planned the new file at
  `src/names/<name>.rs`, proved it on the scratch copy, then `--write` failed with `[CP0001] No such file or
  directory`: the write did not make the folder ([src/gate.rs#L65-L72 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/gate.rs#L65-L72), [src/transform.rs#L1091 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L1091)). Six cases.
- `EditSet::write` makes the folder a new file sits in ([src/gate.rs#L65-L76](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/gate.rs#L65-L76)).

### A crate with only a binary, and a tree nothing checked

- Was: `stored::main` in the `cli` crate landed in package `stored` (the binary's crate root), which has no feature
  model, so the check covered 0 packages; the postconditions were then read on the tree as it was, never written,
  and `add_attribute`, `add_test`, `anchor` came back not proved ([src/transform.rs#L1031 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L1031)). Three cases.
- The landing's package is the crate root's package (custodian_kit's `change::package_of`), so `cli` is checked.
  A change no package checked is never proved, and its postconditions are read on a copy the edits were written to
  ([src/transform.rs#L1533-L1549](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1533-L1549), [src/transform.rs#L1563](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1563); was [src/transform.rs#L1046 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L1046)).

### Every call of a changed signature

- Was: a call was found only where syn's visitor met klepto's place for it, and klepto places a method call at its
  receiver, not its name; a call inside a macro's tokens (`println!("{}", api::open("x"))`, `assert_eq!(set.contains(c), ..)`)
  was never met; and a method's `self` counted as a parameter, so adding one to `contains(self, c)` looked like no
  addition ([src/transform.rs#L338 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L338), [src/transform.rs#L740 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L740)). Two cases.
- The visitor matches a method call at its receiver or its name, and only a call that names the function
  ([src/transform.rs#L338-L376](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L338-L376)); a call klepto resolves inside a macro's tokens is found in the text from
  klepto's place, its argument list matched by parentheses ([src/transform.rs#L378-L493](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L378-L493)); the receiver is not
  a parameter ([src/transform.rs#L1000](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1000), [src/transform.rs#L1042](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1042)).

### A moved item

- Was: `move` appended the item where it lands and kept the import of it there (`pub use within::{.., ProofKind, ..}`
  in lib.rs): "the name `ProofKind` is defined multiple times" ([src/transform.rs#L840 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L840)). Two cases. And the
  private imports the item used (`use big_bad_bool::bitpack`) did not come with it.
- The destination's imports of the item are dropped, a group losing only that name ([src/transform.rs#L1382-L1408](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1382-L1408));
  the moved item takes the private imports of the module it left that it names, unless the destination declares or
  imports the name (klepto's view, which sees what macros declare) ([src/transform.rs#L1271-L1378](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1271-L1378)).

### Names, macros, tests, receipts

- A dependency's name reads with `-` and `_` as one (`big-bad-bool` is `big_bad_bool`) ([src/transform.rs#L1438](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1438);
  was [src/transform.rs#L938 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/transform.rs#L938)). One case.
- A struct under an attribute macro that rewrites it (`#[bitpack]`) is refused for `add_field` and `add_constructor`
  by the macro's name, `[CUST0040]`, before anything is planned ([src/transform.rs#L538](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L538)). Three cases.
- The proof compiles the tests: each check is `cargo check -p P --lib --bins --tests` (no `--lib` for a package
  without one), so a change inside a test module fails at transform time, not at the first `cargo test`
  ([src/lines.rs#L568](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/lines.rs#L568), [src/transform.rs#L1499](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L1499); was [src/lines.rs#L557 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/lines.rs#L557)).
- With a job open in the project, a run's receipt is the job's, a dry run's too (the kit job's dry runs #116 to #119
  landed outside it) ([src/lines.rs#L1286](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/lines.rs#L1286); was [src/lines.rs#L1265 at 0.4.2](https://github.com/jameshuntt/code_passenger/blob/cdb2ace7d8629eacfba1771255ad564b9f0736f7/src/lines.rs#L1265)).

### inline and wrap

- `inline` ([src/transform.rs#L847-L938](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L847-L938)): custodian_kit's `inline_blockers` first; the function's parameters and
  body as they are written; each call, wherever klepto resolves it, from its path to its closing parenthesis,
  becomes `{ let <pattern>: <type> = <argument>; .. <body> }`, in parentheses where it starts a statement and more
  of the expression follows; the function is removed (its docs and attributes with it) when it is not `pub`.
  Postconditions `calls <id> 0` and, removed, `absent <id>`.
- `wrap` ([src/transform.rs#L939-L966](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/src/transform.rs#L939-L966)): the spec's pieces, then the body as it was as the block's value, moved
  one level in unless a string in it runs over lines.
- Both are reversible through the record, as every operation is: `transform --revert`.

### Proof

- tests/transform_more.rs, on the new fixture tests/fixtures/transform_more (`shop` with `stock.rs` and a `depot` that
  imports from it, `cli` with only a binary, `packed` under `#[keep]` from `shop-marks`):
  [tests/transform_more.rs#L37](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L37) the folder made, and both reverts take the files away;
  [tests/transform_more.rs#L56](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L56) `cli` checked, the call inside `println!` given its argument;
  [tests/transform_more.rs#L68](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L68) nothing checked, never proved; [tests/transform_more.rs#L79](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L79) a method's calls by
  receiver, one inside `assert_eq!`, `args .. 3`; [tests/transform_more.rs#L90](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L90) the import dropped and
  `BTreeMap`'s carried; [tests/transform_more.rs#L102](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L102) `dependency shop shop-marks` holds;
  [tests/transform_more.rs#L110](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L110) `[CUST0040]` with `#[keep] (shop_marks::keep)`, a method written;
  [tests/transform_more.rs#L123](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L123) a test that does not compile is not proved; [tests/transform_more.rs#L134](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L134) the
  receipt is the open job's; [tests/transform_more.rs#L153](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L153), [tests/transform_more.rs#L168](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L168) inline, its revert,
  its refusal on `return`, a `pub` function kept; [tests/transform_more.rs#L182](https://github.com/jameshuntt/code_passenger/blob/49d4d5b53b0f3d39d065afc1c52b60295b35cbdb/tests/transform_more.rs#L182) wrap and its revert.
- tests/transform.rs: the gate case now gates `label`: with the tests compiled, gating `version` breaks the golden
  module that calls it, as it should.
- The exercise again on the same three projects with these builds: the 22 findings are down to the refusals that are
  right (see custodian 0.3.2).

## 0.4.2 (2026-10-19)

The writer for custodian_kit 0.3.2's two new operations, which the first
real bounded job in the kit needed. Additive: nothing 0.4.1 does changes.
Every link is pinned to 5c96e39.

- `replace_body`: the function's body, from its opening brace to its closing
  one, becomes the spec's pieces rendered by `change::render_body`; the
  signature, docs and attributes above it are not touched
  ([src/transform.rs#L682](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/transform.rs#L682)). A spec whose function is not the target's own
  name is refused. What a fresh scan must show: the function still takes as
  many arguments as before, and the body's first line is in the file. A body
  of toolkit pieces gives the crate its simple_impl dependency, as
  `add_method` does.
- `change_dependency`: each line `change::dependency_changes` lists is
  changed through toml_edit ([src/transform.rs#L708](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/transform.rs#L708)). Every dependency table
  of the manifest is read: `[dependencies]`, `[dev-dependencies]`,
  `[build-dependencies]` and each target's, and an entry renamed with
  `package = ..` too ([src/commands/edit.rs#L276](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/commands/edit.rs#L276)). A version alone makes the
  entry the registry's: the path is dropped, and an entry with nothing else
  on it becomes a bare `"x.y"` with its comment kept; any other key on the
  entry stays ([src/commands/edit.rs#L234](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/commands/edit.rs#L234)). An entry that takes its source
  from `[workspace.dependencies]` is refused. What a fresh scan must show is
  each manifest naming the source asked, read back with `dep_source`
  ([src/commands/edit.rs#L309](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/commands/edit.rs#L309), [src/transform.rs#L942](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/transform.rs#L942)).
- A change landing in several crates is checked over all of them and every
  crate that depends on any of them, not over the first
  ([src/transform.rs#L1006](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/transform.rs#L1006)).
- `add_test` places the test in the inline module `to` names, and refuses
  when the file has no such module ([src/transform.rs#L653](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/src/transform.rs#L653)).
- custodian_kit 0.3.2 ([Cargo.toml#L48](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/Cargo.toml#L48)).

Proof: [tests/transform.rs#L257](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/tests/transform.rs#L257) (a method's body and a free function's
body replaced, one of them toolkit pieces with simple_impl added; the
signature kept, one line more and nothing else moved; another function's
name refused; a test placed in the fixture's `golden` module, an absent
module refused) and [tests/transform.rs#L324](https://github.com/jameshuntt/code_passenger/blob/5c96e3995995bfa8e62f7ab50f08a16bbd4e945b/tests/transform.rs#L324) (the fixture's path
dependency changed and checked by cargo; the text rule over a dependency
table, a dev table, a target table and a renamed package; `workspace = true`
refused). All 49 tests pass, and the vocabulary test holds that every
operation has its test.

## 0.4.1 (2026-10-19)

code_passenger on custodian_kit 0.3.0. In 0.3.0 the kit's journal is
receipt_history's, and its `put_job` returns receipt_history's error. That
error converts into the kit's `CustodianError`. The two job-row writes now
convert it before they read it as the passenger's error: [src/agent.rs#L226](https://github.com/jameshuntt/code_passenger/blob/112543105d98754d2be57decafee29165021e4e6/src/agent.rs#L226)
and [src/agent.rs#L279](https://github.com/jameshuntt/code_passenger/blob/112543105d98754d2be57decafee29165021e4e6/src/agent.rs#L279). The custodian_kit requirement is 0.3.0
([Cargo.toml#L48](https://github.com/jameshuntt/code_passenger/blob/112543105d98754d2be57decafee29165021e4e6/Cargo.toml#L48)). Nothing else changes. Every link is pinned to 1125431.

Proof: `cargo test`, the 47 tests of 0.4.0, against custodian_kit 0.3.0.

## 0.4.0 (2026-10-19)

custodian's bounded jobs write no code by hand: the agent writes a change
spec, custodian decides it in a preflight, and this release is the writer
that makes it. Additive: nothing 0.3.0 does changes (public API: 314 items
added, none removed, rustdoc JSON of 007fcae against 754a4c8). Every link is
pinned to 754a4c8.

### transform

- `code-passenger transform --spec S [--write] [--out RECORD] [--preflight N]`
  and `transform --revert RECORD`, a declared line like the others
  ([src/lines.rs#L493](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/lines.rs#L493)). The spec is custodian_kit 0.2.0's
  `change::ChangeSpec`; where it lands is the kit's `change::landing`, the
  same rule the preflight reads, and the text written is the kit's rendering
  (bodies as simple_impl 0.1.11 toolkit lines, whole functions and trait
  impls from its generators). This crate finds the exact place with syn (the
  item's span from klepto's line, [src/transform.rs#L205](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/transform.rs#L205)) and plans each
  operation ([src/transform.rs#L399](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/transform.rs#L399)). The match over the kit's 20
  operations is exhaustive, so a word with no implementation does not
  compile; `every_operation_of_the_vocabulary_has_its_test_here`
  ([tests/transform.rs#L239](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L239)) holds that each has a test.
- Cascades, where the operation says so: a new field's default goes into
  every struct literal klepto resolves, across crates
  (`add_field_with_a_default_fills_every_struct_literal_across_crates`,
  [tests/transform.rs#L146](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L146)); a new parameter's argument into every call
  (`change_signature_passes_the_new_argument_at_every_call_and_rename_follows_every_use`,
  [tests/transform.rs#L211](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L211)); a moved item keeps its old path through a
  re-export (`move_and_extract_keep_the_old_paths_through_a_re_export`,
  [tests/transform.rs#L224](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L224)). A match without a catch-all is listed and
  never edited (`add_variant_lists_the_match_it_breaks_and_writes_nothing_unproved`,
  [tests/transform.rs#L158](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L158)).
- Nothing is written before it is proved ([src/transform.rs#L926](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/transform.rs#L926)): the
  edits are checked by cargo on a scratch copy over klepto's check matrix of
  the crate they land in and of every crate depending on it, and a fresh
  klepto scan of the copy must show the facts the operation promises
  ([src/transform.rs#L880](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/transform.rs#L880)). With `--write` the tree is checkpointed in
  the passenger store before and after and the record (custodian_kit's
  `transform::TransformRecord`) is kept; `--revert` puts every file back
  ([src/transform.rs#L1060](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/transform.rs#L1060)). `make_struct_writes_the_declaration_and_revert_puts_the_file_back`
  ([tests/transform.rs#L39](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L39)); a body of toolkit pieces compiles and its
  declared test passes: `a_method_whose_body_is_toolkit_pieces_compiles_and_its_declared_test_passes`
  ([tests/transform.rs#L71](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/tests/transform.rs#L71)).
- A refused spec is `[CP0070]`, exit 2 ([src/error.rs#L80](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/error.rs#L80)).
- `passenger diff --file F` prints the lines a file changed between two
  checkpoints, `lines: n,n` ([src/passenger/command.rs#L80](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/passenger/command.rs#L80)): the lines a
  bounded job's close reads.
- `add_dependency` runs through `dep add`'s own plan
  ([src/commands/edit.rs#L141](https://github.com/jameshuntt/code_passenger/blob/754a4c8e78ff03ccea6b05aae32971dd787de1e8/src/commands/edit.rs#L141)).

### the rest

- custodian_kit 0.2.0; syn and proc-macro2 (`span-locations`), which klepto
  builds on; simple_impl 0.1.11 as a dev-dependency, so the tests compile the
  toolkit lines a spec renders. simple_impl_derive 0.2.7, xccute_contract
  0.1.3.
- Every new file carries the notice Copyright © 2026 1o. All rights
  reserved.; the existing notices are as they were.
- 47 tests: 12 in tests/transform.rs, and 0.3.0's.

## 0.3.0 (2026-10-19)

0.2.0 planned one package at a time from a klepto.toml spec. 0.3.0 plans a
whole workspace from one features document, says what each feature takes
away when it is off, writes headers from that plan, keeps the history of
every write, runs the work as an agent job, and declares every command as an
xccute line. The 0.2.0 `gate` loop is unchanged; its 17 tests pass as they
were.

### klepto 0.3.1 and custodian_kit 0.1.0, from crates.io

- klepto 0.3.1 and custodian_kit 0.1.0 by version, no path dependencies;
  klepto is the one custodian_kit reads with, so a parse made here is the
  kit's type.

- klepto 0.3.0 moved the knowledge-graph export under `klepto::receipt`.
  The receipt command imports it from there.
  Old: [src/commands/receipt.rs#L5](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/commands/receipt.rs#L5).
  New: [src/commands/receipt.rs#L5-L6](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/commands/receipt.rs#L5-L6).
  Proof: `receipt_and_kg_are_written` and the other 16 tests in tests/gate.rs.

### The scanning is klepto's

- The regex scanner (scanning/helpers.rs, the scanning/patterns.rs registry),
  the regex detectors (packs/rust/detectors.rs, `build_usage_set`) and
  `scope_label_from_header` had no caller since 0.2.0. They are kept on the
  branch `kept/pre-0.3.0` and are out of the tree, with the regex dependency.
  Old: [src/engine.rs#L22-L59](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/engine.rs#L22-L59),
  [src/packs/rust.rs#L14-L43](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/packs/rust.rs#L14-L43).
- cargo.rs no longer parses a Cargo.toml. A package's manifest facts are
  klepto's feature model, and the manifest of a package is cargo metadata's
  (klepto's `PackageInfo`), not a walk for a matching `[package].name`.
  `scan` reports the files the scan read instead of a walk of `{root}/src`.
  Old: [src/cargo.rs#L31-L126](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/cargo.rs#L31-L126),
  [src/gate.rs#L108-L128](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/gate.rs#L108-L128),
  [src/engine.rs#L131-L141](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/engine.rs#L131-L141).
  New: [src/cargo.rs#L25-L56](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/cargo.rs#L25-L56),
  [src/gate.rs#L108-L113](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/gate.rs#L108-L113),
  [src/engine.rs#L76-L123](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/engine.rs#L76-L94).
  Proof: `existing_scan_still_works_on_the_fixture_package`,
  `apply_produces_expected_edits_without_touching_the_tree` (tests/gate.rs).

### Workspace features

- One document at the workspace root, `v1.workspace.1o.features` (1o), its
  schema declared once in schema/features/v1.schema.json and checked by
  core-1o's validator; `features.toml` is accepted for the same document and
  is checked against the same schema through its 1o rendering. Both twins
  read into one form. The README's table of keys is rendered from the schema.
  New: [src/doc.rs#L132-L142](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/doc.rs#L132-L142),
  [src/doc.rs#L224-L229](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/doc.rs#L224-L229),
  [src/doc.rs#L321-L343](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/doc.rs#L321-L346).
  Proof: `the_twins_read_into_one_model`, `the_schema_refuses_what_it_does_not_declare`,
  `the_readme_table_is_the_schemas` (src/doc.rs).
- `features plan | apply | check` plans every crate in one run: each crate's
  klepto spec derived from the document, klepto's plan per crate, then the
  cascade across crates (a crate using an item another crate gates forwards
  its feature, `store/log`, and its user is gated the same way; a
  module-level `use` gets a use gate), then one cargo check of every touched
  crate and every crate using one, on one scratch copy holding the whole
  workspace's edits. 0.2.0's cascade stopped at the package
  (klepto plan.rs, the check at line 242) and its oracle copied one package's plan.
  New: [src/workspace.rs#L197-L243](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/workspace.rs#L197-L262) (derive),
  [src/workspace.rs#L344-L408](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/workspace.rs#L344-L417) (the cascade),
  [src/workspace.rs#L483-L544](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/workspace.rs#L483-L547) (the cargo check),
  [src/oracle.rs#L100-L105](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/oracle.rs#L100-L105).
  Proof: `one_plan_for_every_crate_with_the_cascade_across_crates_proven_by_cargo`
  (6 cargo check jobs, converged in one round) and
  `without_the_cross_crate_gate_app_does_not_compile` (tests/features.rs).
- A gate's condition drops a feature that only reaches another it names:
  klepto gives `any(feature = "default", feature = "log")` when `default`
  turns on `log`; the gate written is `feature = "log"`.
  New: [src/workspace.rs#L285-L304](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/workspace.rs#L285-L307).
  Proof: the gate texts in `one_plan_for_every_crate_...`.
- `--write --no-cargo` no longer writes an unproven plan. It refuses with
  `[CP0021]` unless `--unproven` says so. The plan flags are one big-bad-bool
  word whose rules flaga_solver checks.
  Old: [src/commands/gate.rs#L84-L91](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/commands/gate.rs#L84-L91).
  New: [src/words.rs#L11-L53](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/words.rs#L11-L58).
  Proof: `the_words_rules` (src/words.rs), `refusals_carry_their_codes` (tests/features.rs).

### Depth and the unavailable list

- With each feature off, per crate: the items unavailable (klepto's
  `affected_by_feature`), their units (item, function, module, whole file,
  whole crate, by custodian_kit's scope: a whole file is one every item of
  which is gated, by the module tree), every use site of a dependency the
  feature turns off with the unit holding it (its line in the tree as it is),
  and the callers in other crates that lose them
  (`affected_by_feature(..).transitive` outside the crate). The plan report
  and `feature-probe` show it; `feature-probe` reviews against the document
  and knows its features before the manifests do.
  New: [src/depth.rs#L126-L205](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/depth.rs#L126-L205),
  [src/commands/features.rs#L386-L410](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/commands/features.rs#L386-L410).
  Proof: `the_unavailable_list_is_exact` (tests/features.rs).

### Headers from the plan

- `annotate` wrote a skeleton: notes and corpus features were set empty for
  every file and `select_gates` had no caller. The header now carries, for
  each feature with an item of the file behind it, the dependencies the file
  uses that it turns on, the units, the items unavailable and the callers
  elsewhere that lose them; a file wholly behind a feature carries it as
  `#![cfg(...)]` (`select_gates`). annotate refuses (`[CP0023]`) while the
  plan has work left, so a header never describes a gate the code lacks.
  Old: [src/engine.rs#L157-L158](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/engine.rs#L157-L158),
  [src/plans.rs#L19-L27](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/plans.rs#L19-L27),
  [src/plans/verbose.rs#L19](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/plans/verbose.rs#L19).
  New: [src/commands/annotate.rs#L54-L88](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/commands/annotate.rs#L54-L95),
  [src/commands/annotate.rs#L99-L143](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/commands/annotate.rs#L99-L141).
  Proof: `apply_writes_every_crate_check_is_idempotent_and_the_headers_are_the_plans`
  (the four headers compared whole, `--check` after `--write` changes nothing,
  and the annotated workspace compiles in every combination).

### History

- The passenger store was opened at the current directory, tracked
  `src` and `Cargo.toml` by a walk, and `checkout` moved HEAD without
  restoring a file. It is now the workspace root's, whatever directory it is
  opened from; it tracks what klepto's scan reads and the document; its
  paths are pathgen's and its digests stable_digest's; `checkout` writes the
  snapshot's files back and refuses (`[CP0042]`) when a tracked file changed
  since HEAD's snapshot. `features apply --write` checkpoints before and
  after the write, with scan.json, analysis.json, the plan and the document.
  Old: [src/passenger/command.rs#L47](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/passenger/command.rs#L47),
  [src/passenger/store.rs#L201-L210](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/passenger/store.rs#L201-L210),
  [src/passenger/store.rs#L317-L362](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/passenger/store.rs#L317-L362).
  New: [src/passenger/store.rs#L117-L120](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/passenger/store.rs#L117-L120),
  [src/passenger/store.rs#L251-L262](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/passenger/store.rs#L251-L262),
  [src/passenger/store.rs#L345-L385](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/passenger/store.rs#L345-L395).
  Proof: `apply_checkpoints_and_checkout_writes_the_snapshot_back` (tests/features.rs).

### Two inits

- `init` writes the document from the current feature models, so planning
  it changes nothing, and copies its schema where the 1o language server reads
  it (`.passenger/config/schema/`, listed in `oneo.project.json`, custodian's way).
  New: [src/init.rs#L25-L66](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/init.rs#L25-L65), [src/init.rs#L83-L107](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/init.rs#L83-L107).
  Proof: `init_writes_the_current_models_and_planning_it_changes_nothing`
  (both twins, before and after the feature exists, and core-1o's workspace
  checking the document).
- `agent init` makes custodian_kit's job layout and writes the `features-apply`
  job: plan, apply exactly that plan (it reads the plan step's receipt and
  refuses, `[CP0025]`, if the code changed since), annotate (it reads the
  apply step's receipt). Each step is a declared line started by
  custodian_kit's executor, its receipt in the journal; the folder moves
  queued → running → done and ends with a closing receipt.
  New: [src/agent.rs#L33-L51](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/agent.rs#L33-L51), [src/agent.rs#L189-L300](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/agent.rs#L189-L301).
  Proof: `agent_init_creates_the_job_layout_and_runs_the_features_apply_job` (tests/features.rs).
- One template per kind. custodian_kit 0.1.0's agent layout writes its own
  `features` template to `.custodian/config/jobs/features.toml`: the
  eligibility reading (public API, dependents, reach), which changes nothing.
  code-passenger's job is the one that edits, so its kind is `features-apply`
  in `features-apply.toml`, and `agent queue` takes it when no `--kind` is
  given. Before, both wrote `features.toml`, the kit's came first, and the job
  could not read it.
  New: [src/agent.rs#L29-L58](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/agent.rs#L29-L58), [src/agent.rs#L337](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/agent.rs#L337).
  Proof: the same test, which also checks the kit's `features.toml` is left as the kit wrote it.
- custodian_kit 0.1.0's `JobFile` and `Receipt` gained fields: a queued job's
  `job.toml` takes the rest by default, and the job-close receipt names its
  runner, `line`.
  New: [src/agent.rs#L158](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/agent.rs#L158), [src/agent.rs#L304](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/agent.rs#L304).

- `passenger checkpoint`, and the checkpoints around `features apply`, keep
  going past a file klepto cannot parse. Before, one such file refused the
  whole checkpoint with `[CP0005]`. Now klepto is asked to skip it
  (`ignore_parse_errors`); the snapshot keeps it by digest like any tracked
  file, marked `unparsed` with the parser's message; the scan beside the
  snapshot leaves it out; the line prints a `note:` naming it and exits 0.
  New: [src/engine.rs#L75-L82](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/engine.rs#L75-L82), [src/passenger/types.rs#L86-L88](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/passenger/types.rs#L86-L88), [src/passenger/store.rs#L291-L295](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/passenger/store.rs#L291-L295), [src/passenger/command.rs#L44-L50](https://github.com/jameshuntt/code_passenger/blob/1d43c576a432136a681979f9ee5832d429f7ceb1/src/passenger/command.rs#L44-L50).
  Proof: `a_checkpoint_keeps_a_file_klepto_cannot_parse_by_digest_and_says_so`
  (tests/features.rs, fixture tests/fixtures/ws_unparsed).

### Declared lines, codes, the bin feature

- Every command is a simple_impl_derive line over xccute, read back from its
  words, validated, run, decided by its own exit statuses, and ended with a
  receipt (kept in a `.custodian` journal when there is one). `--root R <command>`,
  the form custodian's facade writes, reads as the same line. The oracle's
  `cargo check` is a declared line too. clap is gone; the program is behind
  the `bin` feature, on by default.
  Old: [src/cli.rs#L10-L143](https://github.com/jameshuntt/code_passenger/blob/2b77ad8c7477f9ccc66fb804827f0d667968a896/src/cli.rs#L10-L143).
  New: [src/lines.rs#L30-L57](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/lines.rs#L30-L57), [src/lines.rs#L729-L936](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/lines.rs#L729-L760),
  [src/lines.rs#L1149-L1189](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/lines.rs#L1149-L1189).
  Proof: `every_command_is_a_declared_line` (tests/features.rs).
- Every error is a liaise code in one table (`CP0001`...); a refusal ends a
  line with 2.
  New: [src/error.rs#L12-L83](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/error.rs#L12-L84).
- `dep add | remove` and `attr place | remove`, the writers custodian routes
  to. `attr` places by the gates' rule, now one function both use.
  New: [src/commands/edit.rs#L132-L281](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/commands/edit.rs#L132-L233),
  [src/commands/edit.rs#L337-L394](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/commands/edit.rs#L337-L394),
  [src/gate.rs#L173-L208](https://github.com/jameshuntt/code_passenger/blob/4f3ab7d1873159da157fbd9c2c588bcef0d6f568/src/gate.rs#L173-L210).
  Proof: tests/edit.rs (exact manifests, exact placements, the lines).

### Tests

34: the 17 of 0.2.0 in tests/gate.rs, 10 in tests/features.rs on the
ws_features fixture (`store` and `app`, `log` spanning both, `logsink` the
dependency it turns off; real cargo, offline), 3 in tests/edit.rs, 4 in the
library (src/doc.rs, src/words.rs).
