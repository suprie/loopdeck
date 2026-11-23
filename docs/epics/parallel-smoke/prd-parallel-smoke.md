---
prd: prd-parallel-smoke
epic: parallel-smoke
status: proposed
description: Verify concurrent phase turns and isolated GitHub draft delivery.
---
# Parallel live smoke test

## Acceptance criteria

The alpha phase creates `smoke-alpha.txt` with exactly `parallel alpha` and a trailing newline. The beta phase creates `smoke-beta.txt` with exactly `parallel beta` and a trailing newline. Each phase verifies its own file, keeps the other marker absent, and opens its own draft PR targeting `smoke/parallel-20261124`. Do not check this file's boxes or stage other files. Production executor bookkeeping will update the boxes in the control checkout.

## Phases

### Phase 1
- [ ] `parallel-smoke/alpha` Add and independently deliver the alpha marker

### Phase 2
- [ ] `parallel-smoke/beta` Add and independently deliver the beta marker
