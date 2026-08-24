# Strata Interview Grinder

Generate complete data science interview questions in the exact format used on the [StrataScratch](https://www.stratascratch.com) platform.

## What It Does

The Interview Grinder creates original, interview-ready questions with:

- **Title** — Short, descriptive name
- **Question** — Business-framed problem with clear output requirements
- **Dataset Preview** — Table schemas with sample data, exactly as shown on the platform

Questions are calibrated against a bank of 100 real StrataScratch questions for style, difficulty, and structure — but every output is 100% original.

## Usage

Just ask naturally:

```
Generate an interview question
```

```
Give me a hard SQL question about customer churn
```

```
Grind a question about window functions
```

The skill activates automatically when it detects interview question creation intent.

### Hotkeys

After each question, use these shortcuts:

| Key | Action |
|-----|--------|
| **Q** 🔄 | Generate a brand new question |
| **R** ✏️ | Revise a specific section |
| **D** 📊 | Regenerate dataset with different data |

## Interview Mode

This skill operates in **interview mode** — it presents the question and data, you solve it. Solutions, hints, and edge case explanations are never shown. Edge cases are silently embedded in the dataset for you to discover.

## Installation

```bash
# Via npx
npx skills add stratascratch/skills --skill strata-interview-grinder

# Via Claude Code
/plugin marketplace add stratascratch/skills
/plugin install interview-skills@stratascratch-skills
```

## Examples

**Easy question** — basic aggregation and filtering:
```
Generate an easy interview question
```

**Hard question on a topic** — advanced joins, window functions:
```
Grind a hard question about ranking employees by department
```

**Company-style question:**
```
Create a medium question in the style of Airbnb interviews
```
