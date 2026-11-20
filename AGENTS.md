# Agents

## No tests

This repository has no tests, on purpose. Do not write any.

- No `#[test]` functions, no `#[cfg(test)]` modules or helpers, no `tests/` or `benches/`
  directories, no `[dev-dependencies]` added for testing, no test steps in CI.
- Do not run `cargo test`; there is nothing to run.
- Check a change by reading the code it touches, `cargo build --release`, and
  `cargo clippy --release --workspace --all-targets` with no warnings. For anything on screen,
  look at it: run the app, or render a scene (`glint --scene <name> --frames 40 --snap out.png`).

## Write like a person

This covers everything someone reads: strings in `i18n.rs`, `README.md`, `CONTRIBUTING.md`,
`packaging/release-notes.md`, commit messages, pull request descriptions and comments in code.
Write the way one person explains something to another, in plain words.

- Say what it does or what changed, and name the thing. "Колёсико прокручивает в полтора раза
  медленнее", not "Улучшена прокрутка".
- Only write what is true in the code. No promised speed, no feature that isn't there.
- One idea per sentence. Short sentences. Active voice.
- No bold lead-in that the rest of the line repeats ("**Быстрее поиск.** Поиск стал быстрее").
- No filler. In English: seamless, enhance, robust, powerful, elevate, leverage, unlock, "improve
  the experience". In Russian: обеспечивает, позволяет, является, осуществляет, данный, в рамках,
  "стало удобнее" or "плавнее" without saying how, "много мелких улучшений".
- No em dashes in English. In Russian use the dash only where the grammar needs it; a full stop or
  a colon usually reads better.
- Don't force things into groups of three. List as many as there are.
- Sentence case for headings and labels, not Title Case.
- In the interface, a label says what the control is. Helper text under a setting stays only when
  it adds something the label doesn't (what the choices differ in, what gets deleted, a limit).
  Never narrate what is plainly on screen.
- Errors and confirmations: what happened and what to do next, in one or two sentences. No
  "Oops", no exclamation marks.

Release notes and every other Markdown file are in English. The interface is the one place with
Russian text, in `i18n.rs`. Open the release notes with one line naming the main change. Then "New:"
and "Fixed:", one bullet per change a user would notice: what was wrong or missing, and what it
does now. Leave out internal refactors. Keep the "Which file to download" part as it is, with the
version numbers updated.
