---
name: research-question-framing
description: >
  Use when a research effort starts vague — narrowing "learn about X"
  into a question that's answerable, decision-connected, and scoped to
  the time available. Triggers: "research X for me", "we should look
  into", "what should the research question be", "this investigation is
  sprawling", "scope this research". NOT the research itself — this
  produces the question that literature-review, survey-design, or
  experiment-design then answer.
---

# Research Question Framing

## Overview

Ten minutes of framing saves days of research: extract the decision hiding behind the topic, pick ONE of the tangled question-types, pin every noun, and gate on answerability. The assumptions register turns week-three derailments into day-one checks.

## When to Use

- A research request arrives as a topic ("look into AI code review") instead of a question.
- An investigation is three weeks in with no convergence — usually a framing failure wearing an effort costume.

**When NOT to use:**
- The research itself — this skill dispatches to the method skills.

## Prerequisites

- Access to whoever wants the research (or their written intent) — framing is negotiation with the requester, not solo wordsmithing.

## The Workflow

1. **Extract the decision hiding behind the topic:** ask "what will you DO differently depending on what this finds?" until it lands on a choice, budget, design, or bet. Research with no downstream decision is legitimate exactly once labeled as timeboxed exploration — the failure mode is decision-research funded as exploration and exploration expected to yield decisions.

2. **Decompose the topic into the question types it's hiding, and pick ONE:** descriptive ("what's the current state"), comparative ("does A outperform B"), causal ("does X produce Y"), predictive ("will Z hold at scale"), design ("how might we") — each demands different methods (`experiment-design` for causal, `survey-design` for prevalence, `literature-review` for state-of-knowledge). A vague topic is usually 4–5 questions of different types tangled; naming them separately is most of the untangling, and the step-1 decision selects which leads.

3. **Sharpen the chosen question until every noun is pinned:** population/context ("for teams like ours" → "for 20–50-eng teams on a monorepo"), outcome variable and its measure (`metric-definition` discipline), comparison baseline, timeframe/conditions. The pin-test: could two people research this independently and recognizably answer the SAME question?

4. **Check answerability against three gates:** evidence exists-or-is-generatable (cheap lookup vs runnable experiment vs data nobody has — know which BEFORE committing); timebox fits the question's weight (a quarter-scale question in a two-day slot gets renegotiated: shrink the question or grow the slot); falsifiability-in-practice — what would a "no" look like, and would the requester accept it? (A question whose every answer routes to the same predetermined action is theater; surface that now, kindly.)

5. **Pre-state what would change the answer — the assumptions register:** the 2–4 load-bearing assumptions ("assumes our review bottleneck is reviewer attention, not CI latency"), each checkable early and cheaply. Half of derailed research is a false assumption discovered in week three that a day-one check would have caught; the register is the first day's checklist AND the scope fence (findings challenging an assumption pause for re-framing, rather than silently bending the question).

6. **Write the one-line brief and socialize it:** *question (pinned) + decision + timebox + evidence types expected + out-of-scope list.* The out-of-scope list (step 2's unchosen questions, parked visibly) is the anti-sprawl mechanism: week two's interesting tangent gets parked against it instead of adopted by drift. Requester signs — the cheap ceremony that prevents week-four's "that's not what I meant."

7. **Re-check the frame at the timebox's midpoint:** still the right question given what's emerged? Legitimate re-framing is announced and re-signed (v2, with the reason); illegitimate re-framing is silent drift. When findings arrive: report against the ORIGINAL question first, tangents second, labeled — the discipline that keeps research trusted enough to keep getting funded.

## Common Rationalizations

| Excuse | Why it doesn't hold |
|---|---|
| "The exec said 'look into it' — so I'll look into it" | Topic-shaped assignments produce museum-building. Two extraction questions found the example's real fork (fund an SRE hire or fix rotations) in five minutes; without them, three weeks of landscape tourism. |
| "A broad question keeps our options open" | 'What's the best architecture?' fails the pin-test — two researchers would answer different questions. Breadth at framing time is vagueness billed as flexibility. |
| "The answer's obviously yes — the research just confirms it" | If every finding routes to the same action, it's theater, and delivering it spends your credibility. The falsifiability gate names it before the timebox does. |
| "This tangent is genuinely important — expanding scope" | Every tangent is genuinely interesting; that's why the deadline arrives with six half-answers. Park it against the out-of-scope list; 'not now' is cheap when the list exists. |
| "We'll validate the premise as we go" | The premise IS the day-one check: the example's 'pages ≈ load' assumption was half-wrong, found in one interview. Week-three assumption archaeology is the most expensive research failure there is. |
| "I'll polish the question first, then show the requester" | Solo-polished questions answer what you inferred, not what they meant. Framing is the negotiation; the sign-off is its receipt. |

## Red Flags

- Research underway with no statable downstream decision.
- The question containing unpinned nouns ("better", "our users", "at scale").
- No timebox, or one wildly mismatched to the question's weight.
- Assumptions never listed; the investigation's premise untested.
- Scope grown twice without re-signing.
- Findings delivered against a question nobody remembers agreeing to.

## Verification

- [ ] The decision extracted and written ("we will do A or B depending on...") — in the brief.
- [ ] Question types decomposed; the chosen one and the parked ones listed.
- [ ] Pin-test passed — an outside reader confirms the question is unambiguous.
- [ ] Three gates checked: evidence path, timebox fit, falsifiability — noted.
- [ ] Assumptions register written with day-one checks assigned.
- [ ] Brief signed by the requester; midpoint re-check scheduled.

## Example

Request from the VP: "look into whether we should be worried about our on-call load." Decision extraction (two questions, five minutes): the real fork was "fund a dedicated SRE hire next quarter, or fix rotation-side?" Decomposition surfaced four tangled questions — descriptive (how bad IS it?), comparative (vs industry?), causal (what drives it?), design (what would reduce it?) — the hire decision selecting descriptive+causal. Pinned: "over the last 2 quarters, what is per-engineer pager load (pages/week, off-hours fraction) across our 3 rotations, and what share traces to the top-5 alert sources?" — answerable from PagerDuty data, one-week timebox, and a "no problem here" answer genuinely acceptable. Assumptions register caught the big one on day one: "assumes pages ≈ load" — a rotation interview revealed HALF the burden was un-paged Slack escalations; re-framed (announced, v2) to include them. Delivered against the brief: load concentrated in one rotation, 61% traceable to two alert sources — decision resolved as "fix alerting first (`alerting-design`), revisit the hire in Q2," and the three parked questions became two later briefs and one deliberate never.

## Related skills

- `literature-review` / `survey-design` / `experiment-design` — the methods the framed question dispatches to.
- `metric-definition` — pinning the outcome variables.
- `prd-writing` — the same problem-before-solution discipline, product-side.
- `note-synthesis` — keeping the investigation's findings claim-shaped.
