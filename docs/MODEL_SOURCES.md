# Pinned inference sources

Verified on 2026-10-15 from Hugging Face's repository API including LFS SHA-256 metadata. Exact revisions, filenames, byte counts, licenses and SHA-256 values are in [`models/manifest.json`](../models/manifest.json). Provisioning uses immutable revision URLs; it never resolves `main` at runtime.

| Purpose | Repository | License / notices |
| --- | --- | --- |
| Whisper Base English | [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) | MIT; retain upstream model/runtime notices |
| Whisper Small English Q8_0 (optional "Accurate" transcription, manual download) | [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) | MIT; retain upstream model/runtime notices |
| EmbeddingGemma 2 740M multimodal | [litert-community/embeddinggemma-2-740m-litert-lm](https://huggingface.co/litert-community/embeddinggemma-2-740m-litert-lm) | Repository declares Apache-2.0; see pinned card and full model provenance |
| Gemma 3 1B IT QAT Q4_0 | [ggml-org/gemma-3-1b-it-qat-GGUF](https://huggingface.co/ggml-org/gemma-3-1b-it-qat-GGUF) | [Gemma terms](https://ai.google.dev/gemma/terms); verified public redistributor, preserving Google provenance |
| Optional Gemma 4 E2B IT Q4_0 | [ggml-org/gemma-4-E2B-it-GGUF](https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF) | Gemma terms; independent manual upgrade, 2,841,481,184 bytes. Initially unprovisioned; user-installed weights were later reused read-only for the 0.1.2 short generation regression. |

LiteRT prefixes and Python `EmbeddingEngine` / `Content.AudioFile` API were checked against [Google's embedding documentation](https://developers.google.com/edge/litert-lm/embedding_models) and exercised using the installed 0.18.0 Python package. No PyTorch, Transformers, Sentence Transformers or generic substitute embedding model is installed.

Runtime sources in `scripts/setup-runtimes.sh`: [whisper.cpp v1.9.5](https://github.com/ggml-org/whisper.cpp/tree/v1.9.5), source-archive SHA-256 `ff1a9053feb509ff9d7729703355541ae9690073a6b1c40eb692c962e0dc1720`; [llama.cpp v0.6.0](https://github.com/ggml-org/llama.cpp/tree/v0.6.0), source-archive SHA-256 `09b36dba235fcac180efff18514da7038e65d8e5b9e4aefa59238468abfdec12`. Both use their upstream MIT licenses. `worker/requirements.lock.txt` records the complete installed CPU Python environment. The app never installs anything at normal startup. Explicit onboarding may install pinned LiteRT packages from PyPI into a private environment and download selected models. Offline wheel import is also supported. No credentials are accepted or logged. C++ engines are included in the Linux package; only maintainers compile them.

Model-weight download sizes are separate from RAM and generated caches. LiteRT can create XNNPACK cache files, including caches for unused modalities. The application passes an XDG cache path; upstream cache creation is not proof that the corresponding encoder is resident. We measure RSS independently and make no mobile/Arm benchmark claim for this x86_64 build.

VoxLoom 0.1.2 applies the small, reproducible `scripts/patch-llama-completion.py` delta after verifying/extracting the pinned llama.cpp archive. The completion frontend parsed `--reasoning off` but omitted `enable_thinking` and `chat_template_kwargs` when formatting the initial prompt. The patch forwards these two fields. Both optimized and generic completion executables are rebuilt from that patched source; upstream copyright/license notices remain intact. No model weights are modified.
