###
---

### code_passenger 0.4.0 — scans a workspace with klepto, then does the writing klepto never does:
### feature gates across every crate, cargo edits, headers, reports, and the history of it all.

###
---

### workspace features (new in 0.3.0)

one document at the workspace root says what every crate's features are.
it is a 1o document, `v1.workspace.1o.features`; `features.toml` is accepted
for the same document (keep one of the two):

```text
{
  features {
    default {
      feature "log"
    }
    feature {
      name "log"
      dep "store:logsink"
      dep "app:logsink"
    }
  }
}
```

```toml
# features.toml, the same document
default = ["log"]

[features.log]
deps = ["store:logsink", "app:logsink"]
```

`feature log` with `dep "store:logsink"` means: crate `store` gets a feature
`log`, and its dependency `logsink` becomes optional behind it. one feature
can stack several dependencies and span every crate. `default` turns the
workspace features on by default in every crate that has them. a crate's own
lines (`crate { name "app" ... }`, `[crates.app]`) replace a list verbatim
where the derived one is not what that crate needs.

```bash
code-passenger init --root .                 # writes the document from the current feature models (planning it changes nothing)
code-passenger features plan --root .        # every crate planned and proven with real cargo check; never writes
code-passenger features apply --root . --write   # the same, written, with a passenger checkpoint before and after
code-passenger features check --root .       # CI: exit 1 while there is work; never writes, never runs cargo
```

what happens, in one run for the whole workspace:

1. each crate's klepto spec is derived from the document.
2. klepto plans each crate's gates: every use of a dependency that is now
   optional gets `#[cfg(feature = "log")]` on its carrier, and everything
   in the crate depending on those gets one too.
3. across crates: when `app` uses an item `store` now gates, `app`'s `log`
   turns on `store`'s (`store/log`) and the item in `app` is gated the same
   way (a module-level `use` gets a `use` gate). this repeats until nothing
   new is gated.
4. the cargo check: one scratch copy of the workspace holds every crate's
   edits, and every crate the plan touches, and every crate using one, is
   checked with every feature combination that matters. a compiler error
   klepto maps to an item becomes a gate and the workspace is planned again.
5. with each feature off, the report says what goes: the items unavailable,
   the unit each one is (a function, an item, a whole module, a whole file,
   the whole crate), every use site with its unit, and the callers in other
   crates that lose them.

exit codes: `0` nothing to do, converged, or written; `1` `check` found work
or the cargo check gave up; `2` refused, with a code: `[CP0020]` something a
person has to decide (a carrier klepto cannot judge, a compiler error no gate
answers), `[CP0021]` `--write --no-cargo` without `--unproven` (an unproven
plan is written only when you say so). nothing is written on 1 or 2.

the document's keys, rendered from its schema (`schema/features/v1.schema.json`,
declared once; a test holds this table to it):

| 1o key | TOML | value | meaning |
|---|---|---|---|
| `features` | `(the document)` | block, required | The one block of the document. |
| `features crate` | `[crates.<name>]` | block | One crate's own lines. Repeats: one block per crate. |
| `features crate default` | `default` | block | The crate's default list as Cargo spells it, replacing the derived one. |
| `features crate default feature` | `default = ["<entry>", ...]` | string | One entry of the crate's default list. Repeats: one value per line, in order. |
| `features crate feature` | `[crates.<name>.features]` | block | One feature's whole list in this crate, as Cargo spells it; it replaces the derived list, in this order. Repeats: one block per feature. |
| `features crate feature enable` | `<feature> = ["<entry>", ...]` | string | One entry of the list: `dep:x`, `x/feat`, `x?/feat` or another feature. Repeats: one value per line, in order. |
| `features crate feature name` | `<feature>` | string, required | The feature. |
| `features crate name` | `<name>` | string, required | The crate, by its package name. |
| `features crate optional` | `optional = ["<dependency>", ...]` | string | A dependency made optional with no workspace feature (Cargo's implicit feature). Repeats: one value per line, in order. |
| `features default` | `default` | block | The workspace features on by default, in every crate that has them. Absent: each crate's default list stays as it is. |
| `features default feature` | `default = ["<feature>", ...]` | string | A workspace feature on by default. Repeats: one value per line, in order. |
| `features feature` | `[features.<name>]` | block | One workspace feature. Repeats: one block per feature. |
| `features feature dep` | `deps = ["<crate>:<dependency>", ...]` | string | `<crate>:<dependency>`: the crate gets the feature, and the dependency becomes optional behind it. Repeats: one value per line, in order. |
| `features feature name` | `<name>` | string, required | The feature's name, the same in every crate. |

`init` also copies the schema to `.passenger/config/schema/` and lists that
folder in `oneo.project.json`, where core-1o's workspace (and so
language-server-1o) looks: the document is checked live while you edit it.

###
---

### headers

```bash
code-passenger annotate --root . --check     # exit 1 while a header would change
code-passenger annotate --root . --write
```

every file gets a header between `//! code_passenger:begin` and
`//! code_passenger:end`, written from the features plan: for each feature
with an item of the file behind it, the dependencies the file uses that the
feature turns on, the units of the file behind it, the items unavailable
with it off and the callers in other crates that lose them. a file wholly
behind a feature carries it as `#![cfg(feature = "...")]`:

```txt
//! feature_name:log
//! deps:["logsink"]
//! scope:["file store/src/sink.rs"]
//! corpus:true
//! unavailable:["store::sink::flush", "store::sink::flush_twice"]
//!
//! ----------------------------------------------
//! CORPUS FEATURES ------------------------------
//!
#![cfg(feature = "log")]
```

a header describes gates the code has, so `annotate` refuses (`[CP0023]`)
while the features plan has work left. without a document it plans the one
`init` would write.

###
---

### history

`features apply --write` checkpoints the workspace into the passenger store
before and after it writes (scan.json, analysis.json, the plan and the
document beside the snapshot). the store is the workspace root's
(`<root>/.passenger/`), whatever directory you run from; it tracks the files
klepto's scan reads and the document.

```bash
code-passenger passenger log --root .
code-passenger passenger checkout S000001 --root .    # HEAD moves, the snapshot's files are written back
```

`checkout` refuses (`[CP0042]`) when a tracked file changed since HEAD's
snapshot (checkpoint first, or `--force`); a file the snapshot does not
hold is left as it is.

###
---

### agent mode

```bash
code-passenger agent init --root .     # custodian's .custodian/ layout, and the `features-apply` job template
code-passenger agent queue --root .    # a features-apply job folder in .custodian/state/jobs/queued/ (--kind features-apply)
code-passenger agent run --root .      # plan → apply → annotate, each step a receipt in the journal
```

one template per kind. `features` is custodian_kit's job, written by its
agent layout: the eligibility reading (public API, dependents, reach) that
changes nothing. `features-apply` is code-passenger's, the job that edits,
in `.custodian/config/jobs/features-apply.toml`.

the `features-apply` job is a folder that moves queued → running → done. its
steps are code-passenger lines started by custodian_kit's executor: the plan
(proven, written into the folder), the apply of exactly that plan (it reads
the plan step's receipt and refuses if the code changed since), and the
headers (it reads the apply step's receipt). each step's receipt is in
`.custodian/data/receipts/`; the folder ends with `closing.md` and
`closing.json`.

###
---

### the writers custodian routes to

custodian never edits a manifest or a source file itself; these do.

```bash
code-passenger dep add app maintenance@0.1.0 --features check,runner      # [dependencies] through toml_edit, formatting kept
code-passenger dep add app maintenance --path ../maintenance               # a path dependency
code-passenger dep add app big-bad-bool@0.1.1 --workspace-table            # the version in [workspace.dependencies], `{ workspace = true }` in the crate
code-passenger dep remove app maintenance [--workspace-table]              # out of the crate (and the table once no member names it)

code-passenger attr place --file app/src/lib.rs --item app::serve --line '#[sensitivity(secret)]'
code-passenger attr remove --file app/src/lib.rs --item app::serve --line '#[sensitivity(secret)]'
```

`dep add` without a version or a path is refused (`[CP0060]`) unless the
workspace table already holds the dependency; adding one that is there
updates its version and adds its features. `dep remove` refuses while a
feature still names the dependency (`[CP0062]`).

`attr place` puts one attribute line on the item at klepto's location of
it, by the rule the gates use: a `#[cfg(...)]` merges into the item's own
cfg (`all(old, new)`) or goes right above the declaration; any other
attribute goes above the item's first attribute, after its doc comments.
The same line already there is nothing to do. `--item` is klepto's id
(`app::store::Engine::new`), or `crate::...` for the file's own package; an
item klepto does not name in that file is refused (`[CP0063]`).

###
---

### transform: a change spec made in the code (new in 0.4.0)

custodian's bounded jobs write no code by hand. the agent writes a change
spec (custodian_kit's `change::ChangeSpec`: one operation, its target, its
boundaries, any body as simple_impl 0.1.12 toolkit pieces), custodian
decides it in a preflight, and this line makes it.

```bash
code-passenger --root . transform --spec .custodian/data/specs/changes/config-digest.toml            # proved on a scratch copy, nothing written
code-passenger --root . transform --spec .custodian/data/specs/changes/config-digest.toml --write --out record.json --preflight 12  # written, its record kept
code-passenger --root . transform --revert record.json                                               # every file it wrote put back
```

every operation of custodian_kit's vocabulary is implemented here, and the
match over them is exhaustive: `make_struct`, `make_enum`, `make_impl`,
`add_method`, `make_fn`, `add_field`, `add_variant`, `add_derive`,
`add_attribute`, `add_module`, `add_dependency` (through `dep add`),
`add_test`, `add_constructor`, `add_trait_impl`, `change_signature`,
`rename`, `move`, `extract`, `gate`, `anchor`, `replace_body` (the body
from its opening brace to its closing one, the signature kept),
`change_dependency` (through toml_edit, every dependency table of every
manifest naming it, the other keys of an entry kept), `inline` (each call
becomes a block binding the parameters to its arguments, then the body; the
function removed when it is not `pub`) and `wrap` (the spec's pieces first,
then the body as it was, as the block's value). custodian_kit says where
each lands and renders the text; this crate finds the exact place with syn
(the item's span from klepto's line), and cascades where the operation says
so: every struct literal gets a new field's default, every call a new
parameter's argument, every resolved use a new name, a moved item a
re-export where it was (its import where it lands dropped, the private
imports it used carried with it). a call is found wherever klepto resolves
it: a binary's, a method's by its receiver, one inside a macro's tokens (in
the text, from klepto's place for it). a match without a catch-all is listed,
never edited. an item an attribute macro rewrites (`#[bitpack]`) is refused
for `add_field` and `add_constructor`, by the macro's name (`[CUST0040]`).

nothing is written before it is proved. the edits are checked with cargo on a
scratch copy, over the check matrix of the crate they land in and of every
crate that depends on it (a binary's package too), the library and binaries
and every target as a test (`--lib --bins --tests`, never `--lib` where there
is none); a fresh klepto scan of the copy must show the facts the operation
promises. a change no package checked is never proved, and its facts are read
on a copy the edits were written to, never on the tree as it is. a new file
in a folder no file has made yet gets its folder. with a job open in the
project, a run's receipt is the job's (a dry run's too). with `--write` the tree is checkpointed in the
passenger store before and after, and the record (every file's digest before
and after, the lines written, the checks, the postconditions) is kept.
`passenger diff --file F` prints the lines a checkpoint pair changed, the
lines custodian reads at a bounded job's close. a refused spec is `[CP0070]`,
exit 2.

###
---

### to-toolkit: a function's body as the toolkit calls it already is

custodian_kit's `patterns` reading lists the functions whose bodies are the
expansion of a simple_impl toolkit macro (`ensure`, `require_some`,
`continue_if`, `getter`, ...), exactly or nearly. this line swaps one of them.

```bash
code-passenger --root . to-toolkit --function shop::Order::total                       # proved on a scratch copy, nothing written
code-passenger --root . to-toolkit --function shop::Order::total --write --out records/  # written through transform, the specs and the record kept
```

the preflight: the body matches exactly (a near match is refused with its
difference named, `[CP0070]`), and the crate depends on simple_impl, or the
first step is an `add_dependency` spec that makes it so. each step is a change
spec `transform` makes (`replace_body`: the matched statements become the
calls, the rest of the body stays as written; a whole-method generator such as
`getter!` takes the function's docs and attributes inside the call). the proof,
on a scratch copy before anything is written: `cargo expand` of the function
before and after is the same in custodian_kit's proof form (its locals named by
where they are bound, since a macro's own names are hygienic, then the normal
form), and `cargo test -p <crate>` passes. with `--write` each step is made by
`transform`, proved again by cargo over the check matrix and recorded.

###
---

### every command is a declared line

every command is declared once as an xccute line (simple_impl_derive), read
back from its words, validated, and decided by its own exit statuses; every
run ends with a receipt. with a `.custodian/` folder the receipt is kept in
its journal, otherwise its digest is printed on stderr. both spellings work:

```bash
code-passenger features plan --root .
code-passenger --root . features plan
```

the program is behind the `bin` feature (on by default, so `cargo install`
keeps working); a library user turns it off with `default-features = false`
and gets no command line.

###
---

### one crate, a klepto.toml spec (0.2.0's loop, unchanged)

```toml
# klepto.toml — move `beta` behind a feature `b` in package `app`
[app]
optional = ["beta"]
default = []

[app.features]
b = ["dep:beta"]
```

```bash
code-passenger gate --root . --spec klepto.toml            # plan + diff, nothing written
code-passenger gate --root . --spec klepto.toml --check    # CI: exit 1 while there is gating left to do
code-passenger gate --root . --spec klepto.toml --write    # prove with real cargo check, then write
```

###
---

### review before you gate

```bash
code-passenger feature-map --root .                           # crates, features, deps, use-site counts
code-passenger feature-sites beta --root .                    # every use site of a dep (or of a feature's deps), by file, with cfg
code-passenger feature-probe beta --root . --spec klepto.toml # what is ungated, what would be gated, what gets lost; never writes
```

`--json` on any of them prints the structure instead of the text.

###
---

### the rest of klepto, on the command line

```bash
code-passenger receipt --root . --out receipt.json --kg kg/   # the versioned receipt + knowledge-graph ndjson
code-passenger lint --root . [--policy policy.toml]           # klepto's analyses under a policy; exit code = verdict
code-passenger patterns --root . --top 20                     # repeated code: extract / parameterize / generic / trait / macro candidates
```

###
---

### notes

- klepto 0.3.1 and custodian_kit 0.2.0 come from crates.io (custodian_kit by
  path too, beside it, when building from source); syn and proc-macro2, which
  klepto builds on, find the spans `transform` splices at.
- the cargo check runs `cargo` offline; path dependencies only, nothing is
  downloaded and nothing outside the scratch copy is touched.
- edits inside macro bodies are out of scope; carriers klepto cannot name
  are listed under `unknown` and a person decides.

###
---

### releasing

Bump `version` in Cargo.toml and give CHANGELOG.md a dated `## <version> (YYYY-MM-DD)` entry, then push to main.
CI (`.github/workflows/ci.yml`) tests on stable and Rust 1.90, runs clippy, and publishes to crates.io
through trusted publishing; a version already on crates.io, or an entry still marked `(unreleased)`, publishes nothing.
