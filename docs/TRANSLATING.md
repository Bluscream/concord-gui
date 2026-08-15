# Translating concord

Translations live in `i18n/` as [Fluent](https://projectfluent.org) files, one
per language. English (`en.ftl`) is the source; everything else is a target.

## For translators

The intended route is **[Weblate](https://weblate.org)**, which hosts libre
projects free of charge at [hosted.weblate.org](https://hosted.weblate.org).
It was chosen over the alternatives because it is the one that actually fits
what this project wants:

| | Weblate | Crowdin | Transifex | Pontoon |
|---|---|---|---|---|
| Free for libre projects | yes | yes | limited | yes |
| Self-hostable | yes | no | no | yes |
| Suggestions and voting | yes | yes | yes | limited |
| Fluent support | yes | partial | partial | yes |
| Licence | GPL-3.0 | proprietary | proprietary | BSD |

Weblate and Pontoon are both libre; Weblate wins on Fluent handling, review
workflow and the fact that it will host us for nothing. Nothing here is locked
in either way - the files are plain text in the repository, so a different
platform, a pull request or a text editor all work.

What Weblate gives a translator:

- **Suggestions and voting.** Anyone can propose a string; others vote; a
  reviewer accepts. Nobody needs commit access to help.
- **Translation memory and glossary**, so the same term is translated the same
  way everywhere.
- **Checks** for common mistakes - missing placeholders, inconsistent
  punctuation, strings that changed in the source since they were translated.
- **Comments** on individual strings, for asking what one means in context.

### Doing it by hand instead

Perfectly fine. Copy `i18n/en.ftl`, translate the right-hand sides, and open a
pull request:

```bash
cp i18n/en.ftl i18n/fr.ftl
```

Then add the language to `src/i18n.rs` - a variant, a `tag`, an `endonym` and
a line in `source()`. Four lines; the tests will tell you if you missed one.

## The format

```ftl
presence-online = Online
action-mute = Mute
```

Left of the `=` is the key, which never changes and is never shown. Right is
the text.

Fluent handles the parts that differ between languages, which is why it was
chosen over gettext:

```ftl
# The English source needs no plural forms of its own for a language that
# has six of them - each translation declares what it needs.
unread-count = { $count ->
    [one] { $count } unread message
   *[other] { $count } unread messages
}
```

Counts are passed as arguments rather than pasted into the string by the
program, so a translation can inflect around them.

## Rules that keep this workable

- **English is the source.** Do not edit `en.ftl` to fix another language.
- **A missing key is not a failure.** It falls back to English, so a partial
  translation is usable immediately - which is the normal state of a language
  being worked on.
- **A key only a translation has is a bug**, usually a typo, and shows as the
  English string with no obvious cause. A test catches it.
- **Keys are dashed and describe where the string appears**: `action-` is
  something the user does, `label-` names a thing, `status-` reports state,
  `warning-` is a risk being explained. That context is often all a translator
  gets, so it should carry weight.
- **Do not translate keys.** Only the text.

## For developers

Add a string by adding a line to `i18n/en.ftl` and using it:

```rust
use concord::t;

let label = t!("action-mute");
let heading = t!("unread-count", "count" => unread as i64);
```

`t!` returns an owned `String`, because a runtime catalogue cannot hand out
`&'static str`. Both front ends read the same catalogue, so a string added for
one is available to the other.

The files are embedded with `include_str!`, so there is nothing to install and
no runtime loading to fail. Changing a translation needs a rebuild - a fair
trade for an interface that cannot end up blank because a file went missing.
