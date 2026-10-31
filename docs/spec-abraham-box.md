<!-- SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# The Abraham Box: David on a small board, with night jobs

Status 2026-10-31: **draft, nothing built.** Project owner: "The Abraham Box is a box that has David as its base. That is why the night jobs: to take
over the read-aloud jobs and run them on the local NPU." Planned hardware: an **RK3588** board with **64 GB RAM**. Figures about speeds are estimates and
**have to be measured** on the board (see "Benchmark") before anything is promised.

The name is already in the code (`com.the-abraham-box.psalm`). Abraham's house: David serves the library, Eliezer (the servant) does the errands.

## Two editions, one box

| | Family edition | Publisher edition |
|---|---|---|
| For | a family | a small publisher |
| Runs | David (library, profiles, apps), night jobs for reading aloud | David plus the PDF-to-EPUB converter, review, OPDS catalogue for customers |
| Main job | audiobooks, music, e-books, read aloud at night | turn print PDFs into checked, accessible EPUBs; publish a catalogue |
| Spec | `spec-ebooks.md`, `spec-read-aloud.md` | `spec-ebooks.md` (conversion, publisher catalogues) and this text |

Both are the same software with different parts switched on: an `edition` setting in `/etc/david.toml`, read the same way as `[features]`
(`features.rs`). Not a hidden feature: `features.rs` is for legally unclear functions, and nothing here is.

## Hardware (to be checked)

- **RK3588**: 4 × Cortex-A76 + 4 × Cortex-A55, Mali-G610 GPU, an NPU of about 6 TOPS (INT8). Low power (a few watts idle), silent with a good heat sink,
  small. *Check*: boards with 64 GB are rare; 16 to 32 GB are common. 32 GB is probably enough (see below); decide after the benchmark.
- **Cooling**: a whole night of full load needs an active cooler. Without it the chip throttles and every time estimate is wrong. The box reads the
  temperature and the clock and logs throttling with the job.
- **Storage**: NVMe for the library; the SQLite database on the same disk is fine (`spec-concurrency.md`).

## Night jobs

Long work (reading aloud, converting, transcribing) runs in a **work window**, by default 23:00 to 06:00, set in the settings.

- Reuses the job queue of `spec-read-aloud.md` (`queued → running → done | failed | cancelled`, one job at a time, resumable per chapter).
- New state **`paused_window`**: at the end of the window the running chapter is finished (or cut at the last finished passage), the job pauses and goes
  on in the next window. A crash or power cut loses at most one chapter.
- The window is a schedule like the weekly schedule of the profiles (same model and editor; the box has its own row).
- An administrator can start a job "now" outside the window (a button with a warning that playback and the web may get slower).
- During the window the jobs run at a lower priority (`nice`, and only on the big cores if that measures better), so a child listening at night is not
  disturbed.
- **Estimate before the start**: David knows the length of the text and the measured speed of the engine on this box (see "Benchmark") and shows
  "about 3 nights" before the job is queued. The estimate in `spec-read-aloud.md` is based on this measurement, not on assumptions.

## Reading aloud on the box

The speed that matters is the **real-time factor**: seconds of computing per second of speech. A 10-hour audiobook in a 7-hour night needs a factor below
0.7. At a factor of 10 the same book takes about two weeks of nights.

| Candidate | Voice cloning | On the RK3588 (estimate, *measure*) | Note |
|---|---|---|---|
| Chatterbox (in Eliezer today, via audio.cpp, MIT) | zero-shot from a short sample | probably much slower than real time on the CPU; an NPU port is uncertain (a multi-stage model with a language-model part) | best voice; first choice where a GPU exists |
| Piper (VITS, ONNX) | no; a voice is fine-tuned from roughly 30 to 60 minutes of recordings, once, on a GPU machine | built for small boards; expected near or faster than real time on the CPU | realistic for parents' voices: a parent reads for an hour once |
| Kokoro and similar small models | no | expected fast | good for books in one neutral voice |

The **engine sits behind the `SpeechEngine` trait** of `spec-read-aloud.md`; which one runs is configuration. Possible set-ups:

1. **Box only**: Piper (fine-tuned parent voices) or a neutral voice, at night.
2. **Box plus GPU station**: the box queues and files; the speaking runs on a machine with a GPU (Eliezer with Chatterbox), reached over the
   OpenAI-compatible interface. The box is still the only place the family deals with.

### NPU

- Rockchip's runtimes (RKNN for general models, RKLLM for language models) are closed binaries from the vendor. Using them is a matter of the AGPL
  (*check*: linking a closed library into AGPL code, or keeping it in a separate process behind an interface; the separate process is the safe way).
- Porting a TTS model to the NPU is research with an open outcome. The plan therefore does not depend on it: CPU first, NPU where it measurably helps.
- The labeller of the converter (`spec-ebooks.md`) is a small language model; RKLLM or llama.cpp on the CPU can run it. Reading in the prompt dominates
  (a page with markup is 1,000 to 2,000 tokens, a book a few hundred thousand), so a book takes hours, not minutes: a night job as well.

## Eliezer on the box

Eliezer is built for NVIDIA and AMD GPUs today. On the box it needs a CPU backend (llama.cpp, which it already uses for language models, *check*) and
optionally an RKLLM backend as a separate process. Text-to-speech on the box goes through the same `POST /v1/audio/speech` contract, with a Piper or
Kokoro backend added to Eliezer, so David does not care which machine speaks.

## Publisher edition

What a publisher gets (details of the converter in `spec-ebooks.md`):

- **In**: the print PDF the publisher has anyway.
- **Out**: an EPUB that passes EPUBCheck, with an Ace by DAISY report and alt texts for images, ready for shops and distributors; and an OPDS catalogue for
  customers who use David (or any OPDS reader). Where the publisher allows it, the catalogue says the book may be read aloud in a family.
- **Review step**: a person at the publisher confirms headings, image texts and the flagged pages before a book is released. The machine does most of
  the work; the publisher is responsible for the result. The review report is kept with the book.
- **Everything is local**: manuscripts and print data never leave the box. For publishers who care about their rights this is a main argument.
- Open: selling the box versus running it as a service (a service loses the "local" argument); support and liability when a faulty EPUB reaches a shop
  (the review step and the check report protect both sides).

Order: first the converter shows on real PDFs (owner provides samples, for example from CLV) how good it is; then one publisher as a pilot; then the box.

## Benchmark (first step, no product code)

A script outside the product code, run on the board:

1. Speech: the same German paragraph (about 1 minute of speech) with Piper (a German voice), Kokoro (if German is available, *check*) and Chatterbox via
   audio.cpp on the CPU. Measure the real-time factor, peak memory, temperature and clock after 10 minutes of continuous load.
2. Labeller: 20 real pages through two small models (CPU and, if available, RKLLM). Measure seconds per page and compare the labels with a hand-labelled set.
3. Result as a table in this document; the owner decides the set-up (box only, or box plus GPU station) and whether 32 GB are enough.

## Open questions

- 32 GB or 64 GB (after the benchmark)?
- Piper voices of the parents: who records, how much, where is the fine-tuning done (once, on a GPU machine)?
- Publisher edition: sale or service, price, support?
- Does the box also run Eliezer's chat models for the voice assistant (`spec-voice.md`), or does that stay on the GPU machine?
