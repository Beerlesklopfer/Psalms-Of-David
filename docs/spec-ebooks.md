<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# E-books: hosting, reading and reading aloud

Status 2026-10-31: **draft, nothing built.** Project owner: "The next step is the EPUB reader: to have books read aloud, and to host the e-books
centrally." Written from a conversation with the owner; statements about outside programs, shops and publishers **have to be checked** before anything is
built on them (marked *check*).

## Goal

1. **Host** the family's e-books on David, like the audiobooks: one library, shared, with the same roles, age limits and releases.
2. **Read** them: in Psalms (later) and in any reading app that speaks OPDS (at once).
3. **Read aloud**: an e-book is a text source for the pipeline of `spec-read-aloud.md`; the result is an ordinary audiobook album linked to the e-book.
4. **Convert**: books that exist only as PDF (for example the free PDFs of small Christian publishers such as CLV) become clean EPUBs automatically.

Not a goal: a shop, selling, and anything that removes or works around copy protection (see "DRM").

## Words

- **E-book**: one book as a text file (EPUB after import). Table `ebook`, because `book` is already the audiobook/album (`0001_init.sql`).
- **Edition**: the files of one e-book (the EPUB, the original PDF if it came as one).
- **Spoken edition**: an audiobook album made from an e-book by the read-aloud pipeline.

## Data model (migration number follows the current last one, 0056 at the time of writing)

- `ebook(id, title, author_id, language, isbn, publisher, min_age, folder, file, source_kind, protection, added_at)`:
  `author_id` reuses `author`; `min_age` works like on `book`; `source_kind` is `upload | folder | link | opds | converted`;
  `protection` is `none | watermark` (protected files are refused, see "DRM").
- `ebook_toc(ebook_id, ordinal, title, href, level)`: the table of contents from the EPUB navigation document.
- `ebook_release` like `book_release`: releases per profile or role; the `VISIBLE` rule of the library applies unchanged.
- `ebook_progress(profile_id, ebook_id, locator, percent, updated_at)`: reading position per profile on the server, like the listening position.
  `locator` is an EPUB CFI (or a Readium locator, *check*); the server stores it and does not interpret it.
- `ebook_spoken(ebook_id, book_id)`: the link from an e-book to its spoken editions (several voices possible).
- Optional, phase 4: `ebook_sync(ebook_id, book_id, track_id, ms, locator)`: where a text passage starts in the audio, so a reader can switch between
  reading and listening at the same place (the passages come from the read-aloud job, which already cuts at sentence boundaries).

Covers and files live in the library tree like audiobooks (`<Author>/<Title>/book.epub`, `cover.jpg`); paths come from the settings, never from constants.

## Ways in (sources)

A fixed list. Every way ends in the same import step (validate, read metadata, refuse protected files, file into the tree).

| Way | What it is | Phase |
|---|---|---|
| Upload | admin web and app, like audio uploads (size limit, queue) | 1 |
| Watched folder | a folder David scans for new `.epub`/`.pdf`, like the audio import | 1 |
| Link | an admin pastes a download link (for example from a purchase e-mail); David fetches it with the existing fetch rules (public addresses only, size limit, redirect limit) | 2 |
| OPDS feed | David subscribes to an OPDS catalogue like a podcast and fetches new acquisitions | 3 |
| Converted | a PDF or another format becomes an EPUB (see "Conversion") | 2 |

Not a way in: unofficial interfaces of shops or streaming services (the private APIs of apps). They break the terms of the service, risk the account,
break with every app update, and usually mean decrypting protected files. A shop with an official download interface for its customers can be added
later as one more adapter of the "Link" or "OPDS" kind.

## DRM

- Files with hard copy protection (Adobe ADEPT, Readium LCP, Kindle formats with DRM) are **detected and refused** at import with a clear message
  ("this book is copy protected; read it in the shop's app"). David never removes or works around protection, and the repository contains no code for it.
  Reason: in Germany §95a UrhG forbids circumventing effective protection and distributing tools for it; a public AGPL repository with such a tool would
  put the project itself at risk (*not legal advice; check with a lawyer if it ever matters*).
- Watermarked files ("soziales DRM") are fine and are imported; `protection = watermark` is shown on the book.
- Amazon: since 2026-01-20 authors and publishers can enable DRM-free EPUB/PDF downloads for Kindle titles (*check per title*); those files are imported
  like any other. All other Kindle books stay in the Kindle app.

## Conversion

### Other formats (MOBI, AZW3 without DRM, FB2, DOCX, RTF, ODT, HTML)

`ebook-convert` from Calibre, run as an external program like 7z and ffmpeg (GPLv3; a separate process, so no conflict with the AGPL). Path and options
from the settings. If Calibre is not installed the format is refused with a message saying so.

### PDF to EPUB

A PDF stores where glyphs are on a page, not what is a heading, a paragraph or a footnote. A plain conversion leaves running heads, page numbers, hard
line breaks and hyphenated words in the text; for reading aloud that is fatal (the voice reads every page number). The converter therefore works per page:

1. **Read lines with layout** (PyMuPDF or pdfminer, *check licence*: PyMuPDF is AGPL, which fits): text, font, size, bold, position per line.
2. **Remove page furniture**: lines that repeat at the same place on many pages (book title, chapter title in the head, page numbers) are running heads.
3. **Structure**: headings from size and weight, paragraphs from indent and spacing, footnotes from small type at the foot of the page.
4. **Hyphenation**: join "Glau-|ben" to "Glauben" but keep "Bibel-|und Gebetsbuch"; decided by a word list built from the book itself plus a dictionary.
5. **Images** (see below).
6. **Build the EPUB**: chapters, navigation document, metadata, cover from page 1 (or the embedded cover), language.
7. **Check**: EPUBCheck (W3C) must pass; Ace by DAISY reports accessibility problems (*check both tools' command lines*).
8. **Text proof**: the text of the EPUB must equal the text of the PDF character for character, except whitespace, removed page furniture and joined
   hyphenations. Every other difference fails the conversion and names the page.

#### The language model labels, it does not rewrite

Steps 3 and 4 are the hard ones for rules ("is this bold line a heading or an emphasised sentence?"). A language model helps there, through Eliezer
(local, nothing leaves the house) or any OpenAI-compatible server from the settings:

- The converter sends **numbered lines with their layout data**, one page plus the last and first lines of the neighbour pages (for paragraphs, footnotes
  and tables that cross a page break).
- The model answers **only with labels** (`412: heading level 2`, `413-420: footnote`, `431: join hyphenation`), as JSON with a schema.
- The converter applies the labels; the text proof (step 8) guarantees that the model cannot change, drop or add a single word. A model that "improves"
  a Bible quotation is caught there.
- A page is a natural unit and fits a small context window: a page with markup is roughly 1,000 to 2,000 tokens, so a local model with 32k context
  sees a page and its neighbours with room to spare. The outline of the whole book (first line of every block with its size) is a second, short pass for
  chapter boundaries and the table of contents; it usually fits into 32k as well.
- Models with a large context window (up to 1M) are a fallback for difficult books when the local pass reports many uncertain labels. Sending a
  publisher's book to an outside service is the owner's decision per book, and for publishers a matter of contract.

#### Images

| Kind | How it is recognised | EPUB | Reading aloud |
|---|---|---|---|
| Ornament (rules, logos, chapter vignettes) | the same image on many pages, or very small | dropped or marked decorative | silent |
| Content (photos, maps, diagrams) | everything else; drawn (vector) graphics are rendered as a crop of the page | kept at its place (position between the blocks on the page), caption = small type directly below | configurable: skip, caption only, or "Figure: …" with the description |
| Text in an image (scanned quote, table as graphic) | a vision model or OCR finds text | OCR text added | read |

Content images get an **alternative text** written by a vision model (Eliezer). It is shown for review before the book is released; for publishers it is
the alt text an accessible EPUB needs (European Accessibility Act, in force for e-books since June 2025; micro-enterprises are exempt, *check*).

Scanned PDFs (no text layer) need OCR first; that is a separate, later stage. Digitally produced PDFs (expected for the publishers named above) need none.

## Reading

- **OPDS catalogue from David** (phase 1): `GET /opds/v1` (OPDS 1.2 Atom; OPDS 2 JSON later, *check which readers support which*), authenticated with a
  device token like the API. Every profile sees only what `VISIBLE` allows. Reading apps such as KOReader can then read from David at once.
- **Reader in Psalms** (phase 3): an EPUB renderer in Flutter (*candidate packages to check*), progress synced to `ebook_progress`, download for reading
  offline (same cache rules as audio), font size and night mode, and a button "listen from here" when a spoken edition exists.
- Age limit and releases apply to e-books exactly as to audiobooks. Whether the weekly schedule (Wochenplan) limits reading time as well is an **open
  question** for the owner (proposal: same switch per profile as for listening).

## Reading aloud

An e-book becomes a job of `spec-read-aloud.md`: one job, one chapter of the EPUB per audio file, chapter titles from the table of contents, the voice
chosen by an administrator. The result is a spoken edition (`ebook_spoken`). How the job reaches the speech engine, the night window and the speed
estimate are in `spec-read-aloud.md`, section "Update 2026-10-31".

Rule 6 of `spec-read-aloud.md` ("the text is never stored on the server longer than the job runs") does not fit a hosted e-book, whose text is on the
server anyway. New wording proposed there: protected books are read aloud for private use in the family only; the spoken edition is never shared or
published; texts that are not public domain and not the family's own are read aloud only when the publisher allows it (see "Publisher catalogues").

## Buying (no shop)

- **"Buy at …" link** on a book or wish: opens a search at a shop by ISBN or title (list of shops in the settings, for example SCM, Thalia, Amazon).
  David sells nothing and takes no commission by default. Reason: commission makes the project commercial, and some speech models (XTTS-v2) are licensed
  for non-commercial use only; deciding that is the owner's call.
- **Wish list**: a child (or any profile) wishes for a book; administrators see the wishes in the admin web and buy, or decline. Fits the existing roles.
- After buying, the file comes in through one of the ways above (upload, folder, link).

## Publisher catalogues (OPDS)

Small Christian publishers (CLV, CSV, EBTC are the examples the owner named; *their current e-book formats, DRM and free titles are not checked*) often
care more about spreading their books than about copy protection, and have no app of their own. Proposal:

- The publisher offers an **OPDS catalogue** of its titles (free titles open, bought titles per customer with a token or a personal feed URL).
- David subscribes to it like a podcast and fetches new titles, with metadata, cover and age recommendation.
- A **licence field** in the catalogue entry says whether the publisher allows synthetic reading aloud in the family (proposed: a link relation or a
  property in the OPDS entry, *check what OPDS offers or whether an extension is needed*). David reads it aloud only when allowed. This turns the grey
  area of rule 6 into a licence.
- Because OPDS is a standard, the same catalogue serves every OPDS reader, not only Psalms. That makes it more attractive for a publisher.
- Bible translations have their own rights holders; their permission is needed separately.

How a publisher produces its EPUBs and catalogue is the subject of `spec-abraham-box.md` (publisher edition).

## API (sketch)

- `GET /api/v1/ebooks`, `GET /api/v1/ebooks/{id}`, `GET /api/v1/ebooks/{id}/file`, `GET /api/v1/ebooks/{id}/cover` (all with `VISIBLE`).
- `GET/PUT /api/v1/me/ebooks/{id}/progress`.
- `GET/POST /api/v1/wishes`, `DELETE /api/v1/wishes/{id}` (a wish belongs to its profile; administrators see all).
- Admin: upload, link import, conversion jobs (`GET/POST /api/v1/conversions`, cancel), review of labels and image texts, release.
- MCP (later, consistent with the other tools): `list_ebooks`, `find_problems` for e-books (missing cover, missing language, failed checks).

## Tests

- Import: valid EPUB, broken EPUB, protected EPUB (refused with the message, nothing filed), watermarked EPUB (accepted, marked).
- `VISIBLE`: a child never sees, downloads or finds through OPDS an e-book above its age or not released; 404 like for audiobooks.
- Conversion with fixtures: a two-column page, running heads, footnotes, hyphenation (German compounds), an ornament, a photo with caption, a vector
  diagram; the text proof fails when a fake labeller drops or changes a word.
- Labeller adapter with a null implementation (rules only) and a fake server, like the fake Eliezer of `spec-transcription.md`.
- OPDS feed validates and pages correctly; a reading app's request with a device token works, without token it is refused.

## Phases

1. Import (upload, folder), data model, protection check, OPDS catalogue from David. Reading happens in existing OPDS apps.
2. Conversion: Calibre formats; PDF to EPUB with rules only, then with the labeller; images and alt texts; review in the admin web. First test with real
   PDFs of a publisher (owner provides samples).
3. Reader in Psalms with synced progress; read aloud from an e-book; wish list and shop links.
4. Publisher catalogues (subscribe, licence field); switching between reading and listening (`ebook_sync`).

## Open questions

- Weekly schedule for reading time: yes or no, per profile?
- Which EPUB renderer in Flutter, and which locator format (CFI or Readium)?
- Which labeller model gives reliable labels on a page (listening test equivalent: a labelled test set of a few real pages)?
- Do publishers want the alt texts reviewed in their own tool, or in David's admin web?
- Keep the original PDF next to the converted EPUB (proposal: yes, for re-conversion with a better converter).
