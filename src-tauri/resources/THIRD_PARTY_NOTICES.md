# Third-Party Notices

Mnemora uses the following open-source scheduling component for the English learning feature:

| Component | Version | Upstream | License |
|---|---:|---|---|
| `rs-fsrs` | `1.2.1` | <https://github.com/open-spaced-repetition/rs-fsrs> | MIT |

`rs-fsrs` is used behind Mnemora's local scheduler adapter. Its serialized types do not cross the IPC boundary or define the learning database contract.

## Bundled Agent Skills

Mnemora bundles the complete upstream Skill directories used by its local Skill
repository. The upstream `SKILL.md`, references, scripts, assets and license
files are kept unchanged; `mnemora.json` only supplies local discovery metadata
such as triggers, default state and the pinned upstream commit.

| Source | Pinned commit | License |
|---|---|---|
| `openai/skills` | `49f948faa9258a0c61caceaf225e179651397431` | Apache-2.0 / MIT, see each Skill directory |
| `mattpocock/skills` | `5b15a47f2d7150f545fbcacbfe381787fc0230dc` | MIT |
| `mindfold-ai/Trellis` | `64e663694201005bc87766ef22de89b8da3d4d79` | AGPL-3.0 |
| `github/awesome-copilot` | `83561bd7d8a46fcda0581aedabdf8eac7cb196b6` | MIT |
| `kepano/obsidian-skills` | `a1dc48e68138490d522c04cbf5822214c6eb1202` | MIT |
| `xwmxcz/papers-skill` | `a64c2eda2c9fc182c96e1409cde267b262dbebde` | MIT |
| `Nandansai08/skillz` | `6571a300abb8e49e7c7520896041734aede52c91` | MIT |
| `K-Dense-AI/scientific-agent-skills` | `390f5146bf3c1877cf15636a3dd7b775e4f0f185` | MIT |
| `obra/superpowers` | `b36e0829c6d0140e93cfef2ca599b1b07d4a7797` | MIT |

The application does not execute bundled Skill scripts implicitly. Tool and
agent execution remains subject to Mnemora's existing permission and tool
dispatch boundaries.

The dependency version is locked by `src-tauri/Cargo.lock`. The complete upstream license text is included below.

## rs-fsrs MIT License

MIT License

Copyright (c) 2023 Open Spaced Repetition

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
