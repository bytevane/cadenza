# Research / Architecture PR Checklist

Use this checklist for PRs that modify research docs, ADRs, source manifests, WIT, schema artifacts, or project architecture.

- [ ] This PR does not copy external documentation or articles verbatim.
- [ ] External sources are recorded as links with purpose and access context.
- [ ] If Codex app-server assumptions changed, schema artifacts/hash were updated.
- [ ] If WIT changed, `abi/expected/` was updated and the change is classified.
- [ ] If secrets handling changed, `SECURITY.md` was updated.
- [ ] If AI collaboration rules changed, `CONTRIBUTING_AI.md` or prompts were updated.
- [ ] If a new architectural decision was made, an ADR was added.
- [ ] No tokens, secrets, auth cache, or private data are included.
