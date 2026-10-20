# Engineering Auditor Vision & Architecture Brainstorm Summary

> Historical, non-normative design motivation. Current schemas, tests,
> `docs/project-reference.md`, and the maintained reference set define the
> implemented product when this document differs.

## Context

The discussion began from a narrow question about whether automated time complexity analysis is theoretically possible, but evolved into a much broader realization: **time complexity is only one small component of software engineering quality**.

The motivating context was an internal application repository's source-analysis
tooling, originally inspired by `agent-slop-lint` and later expanded into a
federated deterministic platform covering many traditional static-analysis
capabilities.

While working on the agent harness and its surrounding infrastructure, it became clear that many of the highest-impact engineering problems were **not** detectable by existing linters, static analyzers, or even agent-slop-lint.

Instead, they represented failures of engineering judgment.

This conversation developed a vision for a next-generation **Engineering Auditor** that extends beyond traditional static analysis.

---

# Motivation

The current tooling successfully detects:

* code smells
* structural complexity
* duplication
* dead code
* dependency issues
* architecture violations
* agent-generated "slop"

However, several real-world examples demonstrated an entirely different category of engineering failures.

Examples included:

## Streaming implementation

An agent implemented streaming support by recalculating deltas and rewriting artifacts after every streamed event.

Implementation was:

* clean
* modular
* typed
* lint-clean

Yet runtime complexity was O(n²).

After redesigning the implementation around incremental state maintenance, runtime became O(1).

No existing static tooling identified the original implementation as problematic.

---

## MCP graph construction

Status requests initially required approximately 100 seconds because each request rebuilt the repository graph.

After moving graph construction to startup and caching it:

* startup performed graph generation once
* status simply read cached data

Latency dropped to approximately 100 ms.

Again, the original implementation contained no structural "slop."

It was simply architecturally inefficient.

---

## Graph cache

The graph cache retained every historical graph ever created.

This eventually reached approximately 1.7 GB.

When loading, the implementation:

* scanned all cache entries
* selected eight candidate graphs
* chose the newest one

Nothing about this implementation violated traditional correctness or style rules.

The implementation was internally consistent.

The problem was that the entire design was irrational.

Only one graph was ever consumed.

The cache should simply contain one latest graph.

---

# Key Realization

The important observation from all examples was:

The code was **technically correct**, **well-structured**, and **lint-clean**, while simultaneously being an objectively poor engineering implementation.

This led to the conclusion that:

Traditional linters answer:

> "Is this code acceptable?"

The desired tooling should instead answer:

> "Is this a sensible implementation?"

That distinction became the defining philosophy of the project.

---

# Correctness vs Engineering Quality

Throughout the discussion an important conceptual distinction emerged.

Correctness asks:

* Does the program work?
* Is it type-safe?
* Does it leak resources?
* Does it violate language rules?

Engineering quality asks:

* Is this implementation economically reasonable?
* Is work unnecessarily repeated?
* Is state maintained sensibly?
* Are resources managed appropriately?
* Is the chosen architecture justified?

The project therefore shifts from "linting" toward "engineering auditing."

---

# Time Complexity Discussion

Initially the discussion centered around complexity analysis.

Important conclusions included:

Exact complexity inference for arbitrary Turing-complete programs is impossible due to undecidability.

However:

Practical complexity analysis remains valuable because:

* most production code terminates
* pathological Halting Problem constructions rarely appear in real software
* conservative upper bounds are useful
* many common algorithms are analyzable

This naturally fits into deterministic tooling.

Complexity analysis becomes one detector among many rather than the primary goal.

---

# Taxonomy of Engineering Problems

The examples discussed revealed several distinct classes of implementation problems.

## 1. Algorithmic inefficiency

Classical asymptotic issues.

Examples:

* O(n²) instead of O(n)
* repeated sorting
* exponential recursion

---

## 2. Constant-factor disasters

Asymptotically identical implementations with vastly different runtime.

Example:

Repeated JSON serialization inside hot loops.

---

## 3. Redundant computation

Repeated expensive work without necessity.

Examples:

* rebuilding graphs
* repeated filesystem scans
* repeated parsing

---

## 4. Cache and lifecycle problems

Examples:

* retaining obsolete cache entries
* missing invalidation
* append-only caches
* stale resource retention

---

## 5. Data structure mismatch

Examples:

Using lists where hash maps are appropriate.

Repeated linear searches.

---

## 6. Architectural inefficiency

Examples:

Multiple serialization/deserialization boundaries.

Unnecessary process boundaries.

Repeated initialization.

These are not traditional complexity problems.

---

# Agent-Native Failure Modes

The discussion identified recurring implementation patterns produced by modern coding agents.

These are different from classical software engineering mistakes.

Examples include:

## Local optimization

Agents optimize individual functions rather than overall workflows.

---

## Recomputation bias

Agents frequently recompute expensive state instead of maintaining it.

---

## Missing lifetime reasoning

Agents rarely reason about how long objects should exist.

This naturally leads to cache growth and resource accumulation.

---

## Composition explosion

Agents create:

wrapper

↓

facade

↓

manager

↓

service

↓

coordinator

where a direct implementation would suffice.

---

## Premature generalization

Agents frequently build:

plugin systems

registries

versioned caches

generic abstractions

before multiple implementations actually exist.

These observations suggest entirely new categories of "agent-native" engineering checks.

---

# Core Vision

The project evolved from being viewed as:

"A better linter."

toward:

"A federated software engineering auditor."

North Star statement:

> A federated software engineering auditor that unifies static analysis, architectural analysis, algorithmic analysis, resource analysis, optimization analysis, and evidence-based AI review into a single engineering-quality evaluation pipeline.

This sentence became the defining vision.

---

# Overall Architecture

High-level pipeline:

Repository

↓

Language frontends

↓

Shared semantic representation

↓

Repository graph substrate

↓

Six analysis layers

↓

Engineering Evidence Graph

↓

Optimization engine

↓

Engineering review

↓

Canonical engineering audit report

One of the most important conclusions was that **every subsystem contributes evidence**, not final recommendations.

The LLM is simply another consumer of this evidence.

---

# Engineering Evidence Graph

One of the strongest architectural ideas developed during the discussion.

Rather than each detector producing isolated findings:

Every detector contributes structured evidence into a common graph.

Examples:

Function relationships

Call frequency

Traversal information

Resource ownership

Cache lifecycle

Runtime measurements

Optimization opportunities

Every higher-level analysis consumes this graph.

This becomes the central abstraction of the system.

The LLM also consumes the evidence graph rather than raw source code.

This significantly reduces hallucination and improves explainability.

---

# Shared Semantic Model

The discussion proposed separating:

Language-specific parsing

from

Language-agnostic reasoning.

Language frontends generate a shared semantic representation.

Higher-level analyses should operate primarily on shared concepts:

AST

CFG

Call graph

Import graph

Symbol graph

Dependency graph

Resource graph

Knowledge graph

Work graph

Optimization graph

This enables detectors to remain language-independent wherever possible.

Language-specific logic is isolated behind adapters.

---

# Language-Agnostic vs Language-Specific Analysis

The architecture intentionally separates analyses into two categories.

Language-agnostic:

* graph analysis
* repository structure
* complexity
* duplication
* architectural reasoning
* resource flow
* optimization reasoning

Language-specific:

* ownership
* borrow checking
* language semantics
* syntax rules
* language-specific dead code
* compiler-specific analyses

Existing language tooling becomes donor implementations behind adapters.

---

# Six-Layer Analysis Model

The project eventually converged on six conceptual layers.

## Layer 1 — Correctness

Traditional correctness.

Examples:

Type checking

Null safety

Dead code

Leaks

Undefined behavior

Language rules

No novelty expected.

Donors:

Clippy

Ruff

Pyright

MyPy

Infer

CodeQL

Semgrep

---

## Layer 2 — Structural Engineering

Most mature deterministic tooling lives here.

Examples:

Cyclomatic complexity

Clone detection

Dependency cycles

Architecture

Coupling

File sizing

Magic literals

Agent-slop

Mostly integration work.

---

## Layer 3 — Algorithmic Engineering

Classical algorithm analysis.

Examples:

Time complexity

Space complexity

Repeated sorting

Repeated traversals

Memoization opportunities

Data structure selection

Compiler research and academic complexity analysis provide many donors.

This area is mature but fragmented.

---

## Layer 4 — Systems Engineering

One of the largest opportunities.

This layer models engineering economics.

Examples:

Resource lifetime

Cache lifecycle

Initialization costs

Serialization churn

Data movement

Repeated graph construction

Resource retention

Representation churn

Startup costs

Many pieces exist across compiler research, profilers, runtime tracing, and systems research, but there appears to be little unified tooling.

---

## Layer 5 — Optimization Opportunities

Rather than simply identifying problems:

Suggest equivalent but cheaper implementations.

Examples:

Replace repeated recomputation with incremental maintenance.

Replace repeated scans with indexing.

Replace repeated sorting with maintained ordering.

Compiler optimization research, equality saturation, superoptimizers, and query planners are conceptual donors.

Each recommendation should be evidence-backed with expected benefit and confidence.

---

## Layer 6 — Engineering Review

The only probabilistic layer.

The implemented form is documented in `layer6-llm-review.md`; it retains this
evidence-consumer boundary while adding typed briefs, trust labels, bounded
source lookup, independent criticism, offline verification, and explicit
operator disposition.

Its purpose is **not** to inspect source code directly.

Instead, it consumes structured evidence produced by Layers 1–5.

Example input:

Graph built five times.

No intervening mutations.

Average runtime twelve seconds.

Consumers: status, lookup, refresh.

Recommendation:

Cache graph once.

The LLM is viewed as a reviewer, not an oracle.

---

# Deterministic vs Probabilistic Philosophy

A major architectural principle emerged.

Deterministic analysis should perform all possible objective reasoning.

The LLM should never replace deterministic algorithms.

Instead:

Deterministic systems produce evidence.

The LLM synthesizes engineering judgment.

This makes the review explainable and reproducible.

---

# Donor Philosophy

The project should not attempt to replace decades of existing work.

Instead:

Prefer mature deterministic donors.

Integrate proven algorithms.

Promote mature algorithms into native implementations.

Retain donors as conformance oracles.

Original engineering effort should focus primarily on:

integration

shared evidence

higher-order engineering analyses

rather than reinventing static analysis.

---

# Existing Tool Landscape

An important comparison emerged.

Current tools largely occupy isolated silos.

Examples:

SonarQube

Structural quality.

---

MegaLinter

Tool orchestration.

---

CodeQL

Semantic correctness.

---

Infer

Correctness plus limited cost analysis.

---

Semgrep

Pattern matching.

---

Profilers

Runtime performance.

---

APM

Production behavior.

No existing project appears to unify these into a comprehensive engineering audit.

---

# True Novelty

The discussion repeatedly emphasized that the innovation is **not** new static analysis algorithms.

Instead, the novelty lies in:

* unifying deterministic analyses
* introducing a common Engineering Evidence Graph
* integrating multiple research domains
* elevating engineering quality above isolated lint findings
* allowing evidence-backed engineering review
* treating software quality as a systems-level inference problem

The architecture itself—not individual detectors—is the research contribution.

---

# Ultimate Goal

The project is not intended merely to determine whether software is correct.

Its purpose is to determine whether software represents a sound engineering implementation.

It seeks to combine deterministic evidence from correctness, structure, algorithms, systems, and optimization into a unified engineering evidence model, then augment that evidence with structured AI-assisted review to produce an audit that answers the question:

> **"Is this implementation an economically and architecturally sensible way to solve the problem?"**

This question became the defining objective of the envisioned Engineering Auditor.
