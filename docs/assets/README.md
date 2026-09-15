# Morrow visual identity

The Morrow mark is a sun rising behind a horizon, with two shorter rules beneath
it. The name means the day ahead, and the tapering rules read as lines of source
code: a language for the programs you write next. The mark carries no lettering,
so it stays legible as a small repository header or avatar.

## Files

| File | Use |
| --- | --- |
| `morrow-logo.svg` | Dark ink mark on a transparent background, for light backgrounds. |
| `morrow-logo-dark.svg` | Light ink mark on a transparent background, for dark backgrounds. |
| `morrow-avatar.svg` | The mark on the deep navy field, for square avatars. |
| `morrow-avatar.png` | 1024×1024 raster of the avatar, for services that reject SVG. |
| `morrow-social.svg` | 1280×640 link-preview card with the wordmark and tagline. |
| `morrow-social.png` | Raster of the card, for GitHub's social preview setting. |

The root README selects between the two transparent variants with
`prefers-color-scheme`. `morrow-avatar.png` is the file to upload where GitHub
asks for an organization avatar; regenerate it from the SVG rather than editing
the raster.

## Colors

| Role | Value |
| --- | --- |
| Sun | `#F2A046` |
| Ink on light backgrounds | `#1F2A37` |
| Ink on dark backgrounds | `#E4EBF3` |
| Avatar field | `#141C28` |

The sun is the only accent; do not recolor it per surface. Keep the mark's
proportions and its surrounding space, and keep the sun's lower edge tucked
behind the horizon rule — a gap there turns the sunrise into a stack of bars.

## Accessibility

Provide the name “Morrow” as accessible text whenever the mark identifies the
project: `alt="Morrow logo"` for README headers, and the `<title>` element the
SVG sources already carry. The mark must not be the only carrier of the project
name in a heading. Ink and field meet WCAG AA contrast in both variants; the sun
is decorative and is never used to convey information on its own.

## Provenance

Hand-authored SVG created on 2026-09-15, at the rename from Fern to Morrow.
These are editable vectors on a 1024×1024 grid, not traced raster art: the mark
is one circle and three round-capped rules, so it can be re-cut at any size.
`morrow-avatar.png` is rendered from `morrow-avatar.svg`, and `morrow-social.png`
from `morrow-social.svg`. The card is the only source that sets type; it asks for
Helvetica Neue and falls back to Helvetica or Arial, so re-render it on a host
that has one of them rather than trusting an arbitrary renderer's substitution.
Like the rest of this repository, the assets use the MIT license.

The README references these files by their canonical `raw.githubusercontent.com`
URLs rather than by relative path. `morrow doc --site` copies only Markdown out
of an extras directory, so a relative image path would resolve to nothing on the
generated documentation site that uses the root README as its landing page.
