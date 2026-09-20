# Writing and publishing documentation

Morrow documents code the way Elixir does with `@doc` and HexDocs: documentation
lives next to declarations as literal Markdown, examples inside it are
executable tests, and `morrow doc` renders everything into a browsable site with
navigation, search and cross-references. The same generator documents this
repository: `cargo xtask docs` builds the guides you are reading, the examples
and the Rust API reference into one site.

## Documenting a module

`@moduledoc` describes a whole source file. It appears once, after the optional
`module` line and before the first declaration. `@doc` precedes one function
clause group, type, newtype, constant or trait. Both take a triple-quoted
literal string; escapes are not interpolated, so Markdown and Morrow code inside
stay exactly as written.

```morrow
module geometry.shapes

@moduledoc """
Two-dimensional shapes and their measurements.

Areas are integers because the runtime targets exact arithmetic first. See
`Shape` for the variants and `area` for the calculation.
"""

@doc """
A closed shape with integer dimensions.
"""
pub type Shape:
    Circle(Int)
    Square(Int)

@doc """
Compute the area of a `Shape`.

# Examples

```morrow
area(Square(3))  # => 9
```

# Errors

Circles round down because π is approximated as 3.
"""
pub fn area(shape: Shape) -> Int:
    match shape:
        Circle(radius) -> 3 * radius * radius
        Square(side) -> side * side
```

`morrow fmt` preserves both attributes and places `@moduledoc` directly below the
module line. Hover in an editor connected to `morrow lsp` shows `@doc` text.

### Markdown in documentation

Documentation is rendered with a bounded CommonMark/GFM subset:

| Feature | Notes |
| --- | --- |
| Headings `#`…`######`, setext headings | Shifted below the page or declaration heading; IDs are generated for linking. |
| Paragraphs, emphasis, `code` | `*em*`, `_em_`, `**strong**`, `~~strikethrough~~`, code spans with any backtick run. |
| Fenced code | ```` ```morrow ```` blocks are highlighted and, when they contain `# =>` expectations, executed by `morrow test --doc`. Other languages render verbatim. |
| Lists, block quotes, rules | Nested lists by indentation, task checkboxes and GitHub-style alerts; tight items render without paragraphs. |
| Pipe tables | Header, delimiter row with optional `:` alignment, body rows. |
| Links | `[text](url "title")`, `<https://…>` and bare `https://` URLs. Only `http`, `https`, `mailto`, `#fragment` and relative destinations become links; other schemes render as text. |
| Reference links and footnotes | Case-insensitive link definitions, numbered footnotes in reference order, and return links. |
| Images | Inline and reference images render with escaped alt text and optional titles. Relative and approved URL schemes follow the link safety policy. |
| Raw HTML | An allowlist retains structural tags and safe attributes, repairs tag balance, and strips scripts, styles, event handlers and unsafe URLs. |

### Cross-references

Inline code that names a declaration becomes a link:

- `` `area` `` links to a declaration in the same module.
- `` `geometry.shapes.area` `` links to a declaration in another module of the
  same site. Modules are addressed by their declared `module` name or, for
  files without one, by their path with `/` replaced by `.` (`lib/math.mr` is
  `lib.math`).
- `` `geometry.shapes` `` links to a module page.
- Types and values keep separate anchors (`#t:Shape` and `#area`), so a type
  and a function may share a spelling.

Links between guides use ordinary relative Markdown paths; `[guide](docs/GUIDE.md)`
is rewritten to the generated page when `GUIDE.md` is part of the site.

### Executable examples

Every ```` ```morrow ```` block in `@moduledoc` or `@doc` is a documentation test.
Lines ending in `# => value` are checked with ordinary pattern matching; see the
[test runner](TEST_RUNNER.md). Run them with `morrow test --doc <source|directory>`.

## Generating documentation

| Command | Output |
| --- | --- |
| `morrow doc lib.mr` | Markdown for one file on stdout. |
| `morrow doc lib.mr --html -o docs.html` | One standalone, script-free HTML page. |
| `morrow doc src --html -o docs.html` | One page for a directory with module navigation. |
| `morrow doc src --inferred …` | Adds signatures resolved by the checker. |
| `morrow doc src --site docs-site …` | A multi-page site: one page per module and guide, a sidebar, local search and a JSON search index. |

### Sites

```sh
morrow doc src --site docs-site \
    --title "Geometry" --version 1.2.0 \
    --extras README.md --extras docs \
    --link "Source=https://example.com/geometry" \
    --inferred --open
```

- `--site <directory>` writes the site there. Missing parent directories are
  created. The directory is created, or replaced atomically when it already
  holds a generated site. Directories that contain anything else, symbolic
  links, files, the documented sources or their ancestors are refused, so a
  stray `--site .` cannot delete a project.
- `--extras <path>` adds Markdown guides: a file, or the `.md` files directly
  inside a directory except `README.md`. Pass a README as its own `--extras`
  file to publish it. Repeat the option to add more. The first `README.md`
  becomes `index.html`; other guides take their file stem as page name. A
  leading `# Heading` (or a raw `<h1>`) becomes the page title.
- `--link label=url` adds sidebar links. Destinations follow the same safety
  rules as Markdown links.
- `--title` and `--version` label the site; the title defaults to the source
  directory name.
- `--inferred` typechecks each module graph and adds checked signatures.
- `--open` launches the platform opener on `index.html` after publication.
- Without a source operand, `--site` documents guides alone.

Every generated file sits in one flat directory, so the site works from disk
without a server: `index.html`, one `<name>.html` per module and guide,
`morrow-docs.css`, `morrow-docs.js` and `morrow-search.js`. The script only filters the
bundled search index, toggles the sidebar on small screens and remembers the
light/dark theme; it never fetches or evaluates data. Press `/` to search.

Limits keep generation bounded: 256 modules and 256 guides, 1 MiB per guide,
8 MiB of source, 16 MiB per page and 64 MiB per site. Page names that collide
(a module `guide.mr` next to `GUIDE.md`) are reported before anything is written.

## Documenting Morrow itself

`cargo xtask docs [output] [--no-rust]` builds the staged compiler and then runs
`morrow doc examples --inferred --site <output>` with this repository's README,
`docs/`, `docs/language/`, design, roadmap, decision record, build guide and style guide as extras.
It copies `docs/assets/` to the same relative path in the output so repository
images resolve when the site is opened locally or hosted.
Unless `--no-rust` is given it also runs `cargo doc --workspace --no-deps` and
copies the Rust API reference to `<output>/rust/`, linked from the sidebar. The
default output is `dist/docs`, which is ignored by git.

The Rust sources keep rustdoc comments (`//!`, `///`) for the compiler, runtime
and tooling internals; the Morrow-facing language and library documentation lives
in `docs/` and in `@moduledoc`/`@doc` attributes.

## Current limitations

- Documentation comments attach to functions, types, newtypes, constants and
  traits. Trait implementations and generated methods are not listed.
- The renderer supports a bounded Markdown dialect, not every CommonMark/GFM
  construct. Raw HTML is sanitized rather than passed through unrestricted;
  unsupported tags and attributes are removed. External images are referenced,
  not downloaded or bundled by the compiler. The repository xtask separately
  copies its local assets.
- Source links (`View source`) are not generated; pages show the module path.
- Search runs in the browser over the bundled index; there is no server-side
  search or versioned documentation hosting.
