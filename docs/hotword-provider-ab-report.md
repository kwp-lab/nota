# Hotword Provider A/B Acceptance Report

- Status: Manual test record
- Tested: 2026-08-25 and 2026-08-26
- Scope: ASR interfaces and transcription effect only; UI excluded

The three tests used the same 16.4-second synthetic Ogg recording. The target
term `珂岚智匣` was spoken three times. Each Provider transcribed the recording
once without hotwords and once with the target term enabled. Counts below are
normalized exact matches in the returned transcription.

| Provider / model | Hotword mapping | Without hotword | With hotword | Result |
|---|---|---:|---:|---|
| DashScope `qwen-audio-3.0-asr-flash-filetrans` | Inline `vocabulary`, weight `4` | 0 | 3 | Passed |
| Nota Server Paraformer SeACo | Decoder `hotword` bias | 0 | 3 | Passed |
| Nota Server Fun-ASR-Nano-2512 | Prompt `hotwords` list | 0 | 0 | No effect observed |

A separate 32-minute real-recording probe for the product name `Busabase`
compared no hotword with DashScope super-hotword weight `50`. Exact normalized
matches increased from 1 to 29. This proves that the super-hotword request took
effect, but the count must still be compared with actual speech before adopting
weight `50` broadly because excessive bias can create false positives.

For Fun-ASR-Nano, both API jobs completed successfully and the two returned
transcriptions were not identical, but neither contained the target spelling.
This result means the current sample did not demonstrate a useful Nano hotword
effect; it does not mean the request was rejected or that the model contract
lacks hotword support. A broader speech corpus should be tested before treating
Nano hotwords as production-verified.

These are opt-in manual acceptance tests and are not part of the regular test
suite. The synthetic audio, API credentials, job data, and transcript bodies
are not committed.
