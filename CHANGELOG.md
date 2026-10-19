# Changelog

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
