# Handover: trimming the test suite

**Branch:** `todo-search` — one commit ahead of `main`, holding only this note.
`main` (`24f3e7b`) already has the `/` and `h` work; nothing here is unmerged
code. Delete the branch once the cleanup below is done or decided against.

## Where it stands

138 tests: 132 run, 6 `#[ignore]` live ones that talk to real Jira.

|  | prod lines | test lines | tests |
|---|---|---|---|
| `todo/list.rs` | 884 | **1,016** | 47 |
| `todo/jira.rs` | 488 | 392 | 27 |
| everything else | 1,976 | 1,303 | 64 |
| **total** | **3,348** | **2,711** | **138** |

0.81 test-lines per production line, which is lean for a TUI. The count is not
the problem; the concentration is.

## The agreed cuts — 138 → 131

Small, and all of them are things that are wrong rather than merely surplus:

1. `pick::an_empty_list_never_starts_a_picker` — **tests the wrong function.**
   Asserts against `run()`, but the empty-list guard lives in `pick()`. Worse
   than no test: it reads as coverage and isn't. Fix it or drop it.
2. `ui::todo::only_item_rows_are_selectable` — **the name is now false.** Empty
   groups *are* selectable; the fixture just happens to have none. It would go
   on passing after a regression.
3. `ui::todo::row_of_walks_items_not_rows` — a 5-line helper, covered
   transitively by `render_tests::every_cursor_position_has_exactly_one_row`.
4. `jira::removing_one_of_several_leaves_the_list_alone` — strictly subsumed by
   `removing_takes_the_item_and_leaves_its_siblings`: same call, weaker asserts.
5. `list::mismatched_brackets_do_not_fire` → fold into
   `a_single_angle_bracket_waits_to_be_doubled`.
6. `list::group_jumps_land_on_the_first_item_of_the_group` → fold into
   `the_cursor_walks_items_across_group_boundaries`.
7. `theme::rejects_malformed_hex` → fold into `parses_six_digit_hex`.

## The bigger win, which isn't deletion

`todo/list.rs` grew four test modules in four separate sittings and each one
brought its own scaffolding:

- `item()` defined **four times** (`tests`, `render_tests`, `search_tests`,
  `hide_done_tests`)
- three near-identical `list()` / `populated()` fixtures
- `press()` twice, and two separate terminal-render helpers
  (`render_tests::render` and `hide_done_tests::screen`)

That's ~150 duplicated lines. Hoisting one shared `mod fixtures` cuts more
lines than deleting all seven tests above, and costs no coverage. Do this
first; the file is the outlier and this is why.

## What was looked at and deliberately kept

- **The five section-scoping tests in `jira.rs`** — they look like a table
  waiting to happen. They aren't: each is a distinct ADF shape (lowercase
  heading, deeper heading, same-level heading, no heading, checkbox outside the
  section) and a table would put a layer of indirection between a failure and
  which rule broke. Same conclusion for the ten nesting tests.
- **The ten `render_tests`** — every bug in the first cut of this pane was a
  rendering bug that `--dump` could not see. These draw real `TestBackend`
  frames. They are the reason the second cut worked.

## The one open question

There are **five footer-string tests**: `the_footer_advertises_the_todo_toggle`,
`a_nested_ticket_advertises_the_way_back`,
`the_checklist_footer_replaces_the_ticket_hints`,
`slash_asks_for_the_picker_and_the_footer_says_so`,
`the_footer_says_which_way_h_points`.

They break on wording, not behaviour. Two are worth defending — the footer is
this tool's only discoverability, and `t` once existed with nothing announcing
it. The other three are a judgement call and were left pending a decision.
