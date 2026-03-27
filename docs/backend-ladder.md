# Backend ladder

## `classical-baseline`
Purpose: stable baseline, deterministic regression target, low integration risk.

## `fast-preview`
Purpose: quick look generation and operator feedback loop.  
Non-goal: final delivery master.

## `seedvr-teacher`
Purpose: heavy offline reference generation or teacher outputs for distillation and comparison.

## `stcdit-studio`
Purpose: default final-output backend.  
Requirements:
- patch-wise 4K support,
- structural guidance,
- temporal QC,
- detail refinement compatibility.

## `helioframe-master`
Purpose: research flagship path that can exceed the studio path, but only if it passes the same visual and temporal gates.
