# 🤖 Codex Execution Guide – Cleanup Daemon Project

This project is **safety-critical** (resource deletion).
All operations MUST follow **fail-closed behavior**.

---

# 🧠 Core Principle

> **Safety > Correctness > Performance > Features**

If uncertain → **DO NOTHING (fail-closed)**

---

# 🔗 External YAML Configs (CRITICAL)

The repository includes structured configs:

* `agents/coder.yaml`
* `agents/reviewer.yaml`
* `agents/tester.yaml`

## YAML Usage Rules (MANDATORY)

Before executing ANY phase, Codex MUST:

1. Identify the relevant YAML file
2. Read the file from `/agents`
3. Apply its rules as an extension of this document
4. Then proceed with execution

### Phase → YAML Mapping

* Implementation → `agents/coder.yaml`
* Review → `agents/reviewer.yaml`
* Testing → `agents/tester.yaml`

If a YAML file exists, it **MUST be read and followed**.

---

# 🔄 Execution Workflow (MANDATORY)

Codex MUST execute tasks in strict phases:

---

## 1. Branch Phase

```bash
git checkout -b <type>/<feature-name>
```

---

## 2. Test Phase (TDD REQUIRED)

* Write failing tests FIRST
* Cover:

  * edge cases
  * safety conditions
  * failure scenarios

---

## 3. Implementation Phase

➡️ MUST load: `agents/coder.yaml`

* Write minimal code to pass tests
* Follow:

  * fail-closed logic
  * strict safety guards
* DO NOT modify unrelated code

---

## 4. Safety Validation Phase

Before proceeding:

* Metadata must be complete
* Resource must NOT be:

  * in use
  * protected
  * referenced
* Policy conditions must pass

If ANY check fails → **SKIP deletion**

---

## 5. Review Phase (Self-Review REQUIRED)

➡️ MUST load: `agents/reviewer.yaml`

Validate:

* No unsafe deletion paths exist
* Edge cases handled
* Code is readable and maintainable
* Comments explain *why* (not just what)

Reject your own code if unsafe.

---

## 6. Testing Phase

➡️ MUST load: `agents/tester.yaml`

Validate:

* Dry-run vs real execution behavior
* No deletion of:

  * running containers
  * referenced artifacts
* Failure scenarios:

  * partial execution
  * backend failure

If ANY test fails → go back to Implementation Phase

---

## 7. Documentation Phase

Update:

* `/docs` → behavior + safety rationale
* `/flowcharts` → workflow or policy changes

Missing documentation = incomplete task

---

## 8. Commit Phase (MANDATORY)

```bash
git add .
git commit -m "<type(scope): description>"
```

Rules:

* Small, focused commits
* MUST include all scoped changes
* No partial implementations

---

## 9. PR Phase (MANDATORY)

```bash
git push origin <branch-name>
```

Then:

* Create PR → `main`
* Include:

  * what changed
  * why it changed
  * safety considerations

---

## 10. Merge Conditions

ONLY proceed if:

* All tests pass
* No safety violations
* Code is complete and documented

---

# 🌿 Branching Rules

* NEVER work on `main`
* One branch = one responsibility

### Naming

* feature/<name>
* fix/<name>
* refactor/<name>
* backend/<name>
* policy/<name>
* safety/<name>

---

# 🛡️ Safety Rules (CRITICAL)

The system MUST NEVER:

* Delete active resources
* Delete referenced artifacts
* Delete protected resources
* Execute without policy validation

If uncertain:

> ❗ SKIP ACTION

---

# 📋 Code Standards

## Required

* Every module MUST explain purpose
* Every safety decision MUST explain:

  * why deletion is safe
  * why skipping occurs
* All edge cases MUST be documented

## Forbidden

* Generic comments
* Missing safety explanations
* Stale comments

---

# 🚫 Forbidden Actions

* Direct commits to `main`
* Skipping tests
* Skipping PR
* Unsafe deletion logic
* Uncommitted changes
* Missing documentation updates

---

# 📁 Documentation Rules

* `/docs` → required for behavior changes
* `/flowcharts` → required for logic/workflow changes

ALL changes must be reflected in docs.

---

# 🔁 Full Lifecycle

1. Create branch
2. Write tests
3. Implement feature
4. Validate safety
5. Review code
6. Test execution
7. Update docs + flowcharts
8. Commit
9. Push
10. Create PR
11. Merge

---

# ⚡ Execution Rules (IMPORTANT)

* Think step-by-step internally
* ALWAYS load relevant YAML before phase execution
* Follow phases strictly (NO skipping)
* Prefer simple, explicit logic
* Always validate assumptions
* Safety checks override all other logic

---

# 🧠 Final Reminder

This is a **production-grade deletion system**.

> If unsure → DO NOTHING
> If risky → FAIL CLOSED
> If incomplete → DO NOT COMMIT

---
Always print "USING AGENTS FILE" before doing anything
