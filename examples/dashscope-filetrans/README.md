# DashScope File Transcription Probe

This isolated example validates the raw HTTP flow needed by a future Nota
DashScope adapter. It does not use the DashScope SDK and is not part of the
desktop runtime.

The probe performs these steps:

1. request a model-bound temporary upload policy;
2. upload the original audio file to DashScope's private temporary storage;
3. submit a `qwen-audio-3.0-asr-flash-filetrans` asynchronous task;
4. persist the task id before polling;
5. download the provider result after success;
6. normalize sentence timestamps and `speaker_id` values into Nota-shaped
   segments.

The sample audio, task state, provider responses, and normalized transcript are
ignored by Git. The console reports only phases, request ids, counts, and file
locations. It does not print credentials, temporary URLs, or transcript text.

## Prerequisites

- Python 3.10 or newer;
- `requests` from `requirements.txt`;
- a DashScope API key with access to
  `qwen-audio-3.0-asr-flash-filetrans`.

The script reads `DASHSCOPE_API_KEY` from the current environment first. Set it
only in the current shell when possible:

```powershell
$env:DASHSCOPE_API_KEY = "your-key"
python -m pip install -r .\examples\dashscope-filetrans\requirements.txt
```

For a repeatable local-only probe, the script can also read this ignored file:

```dotenv
# examples/dashscope-filetrans/.env
DASHSCOPE_API_KEY=your-key
```

Never commit the `.env` file or pass the key as a command-line argument.

## Run

From the repository root:

```powershell
python .\examples\dashscope-filetrans\transcribe.py `
  .\examples\dashscope-filetrans\sample.ogg
```

Use a known speaker count when desired:

```powershell
python .\examples\dashscope-filetrans\transcribe.py `
  .\examples\dashscope-filetrans\sample.ogg `
  --speaker-count 2
```

The default enables diarization and automatic language detection. A supplied
speaker count must be from 2 through 100, matching the provider contract.

## Resume after interruption

After the task id has been saved, stop the script with Ctrl+C and resume it:

```powershell
python .\examples\dashscope-filetrans\transcribe.py `
  .\examples\dashscope-filetrans\sample.ogg `
  --resume
```

If the process stopped while the submit request was in flight but before its
response was saved, the script cannot know whether DashScope created a billed
task. It refuses to resubmit by default. `--allow-ambiguous-resubmit` exists for
an explicit test-only retry after accepting the duplicate-task risk.

## Manually verify that hotwords change recognition

`verify_hotword_effect.py` is an opt-in A/B acceptance probe. It uploads one
recording, creates one transcription without hotwords and one with DashScope's
inline `vocabulary`, then checks only whether the expected spelling occurs more
often in the hotword result. The default weight is `4`; `--weight 50` tests a
super hotword. It does not print or save transcript or hotword text. The regular
test suite never discovers or runs this script.

Use a short test recording in which a rare name or product term is normally
misrecognized. Running the probe creates **two potentially billable DashScope
tasks**, so an explicit acknowledgement flag is required:

```powershell
python .\examples\dashscope-filetrans\verify_hotword_effect.py `
  .\examples\dashscope-filetrans\sample.ogg `
  --hotword "目标热词" `
  --expected "期望在转写中出现的写法" `
  --confirm-two-paid-tasks
```

Repeat `--hotword` and `--expected` to check more than one term. When the
expected spelling is already present in the baseline with the same frequency,
the probe exits with code `2` (`INCONCLUSIVE`) instead of claiming that the
hotword caused the result. A missing expected spelling exits with code `1`; a
demonstrated improvement exits with code `0`.

To test whether the product name `Busabase` benefits from a super hotword:

```powershell
python .\examples\dashscope-filetrans\verify_hotword_effect.py `
  C:\path\to\recording.ogg `
  --hotword "Busabase" `
  --expected "Busabase" `
  --weight 50 `
  --confirm-two-paid-tasks
```

DashScope accepts ordinary weights `1` through `5`, or `50` for a super
hotword. The probe still creates two potentially billable tasks so it can prove
an improvement over the same recording without hotwords.

## Local outputs

- `state.json`: local checkpoint with the audio fingerprint, temporary object
  reference, expiry, and task id;
- `output/task-response.json`: final task response;
- `output/provider-result.json`: downloaded provider transcription result;
- `output/nota-transcript.json`: normalized transcript shape;
- `output/error-response.json`: response body from a failed HTTP operation,
  when available.

These files can contain private paths, provider URLs, or transcript content.
Do not commit, upload, or attach them without reviewing and redacting them.

## Provider lifecycle caveats

- DashScope temporary objects expire automatically after 48 hours and cannot
  be deleted early through this flow.
- Requests using an `oss://` input require
  `X-DashScope-OssResourceResolve: enable`.
- The upload policy response supplies the authoritative maximum upload size;
  this probe checks it before uploading.
- Provider task results have a limited retention window, so the result is
  downloaded and committed locally immediately after success.
- When diarization is enabled, the provider recommends keeping audio within
  two hours.

Official references:

- <https://platform.qianwenai.com/docs/api-reference/speech-recognition/fun-asr-recording/restful-api>
- <https://platform.qianwenai.com/docs/api-reference/more/upload-file-get-temporary-url>
