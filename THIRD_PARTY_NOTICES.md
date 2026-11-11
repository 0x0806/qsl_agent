# Third-party notices

QSL Agent is licensed under the [QSL Team Source-Available License v1.0 (2026)](LICENSE).
The following third-party material is bundled with it under its own licence. This file and the
copy of the licence text beside the skill files must stay with any copy of that material.

## QSL Security Audit skill (`src/skills_bundles/qsl-security-audit/`)

The methodology, hunting playbooks, JSON schema and validators of the bundled **QSL Security Audit**
skill are derived from the MIT-licensed `security-audit` skill at
<https://github.com/cloudflare/security-audit-skill> (commit `c1c8a8c1471069fb0e188eeaff69b8e8db6564a8`).
Modifications by Quantum Secular Labs: frontmatter moved into the QSL skill registry, the
"Platform terminology" section rewritten for QSL Agent's tools and limits, and the default output
directory changed. The validators (`*.cjs`), `report-schema.json` and the companion Markdown files
are otherwise unmodified.

```
MIT License

Copyright (c) 2025-2026 Cloudflare, Inc.

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
```
