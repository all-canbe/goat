"""Unified CLI entrypoint for kz-skill-creator."""

from __future__ import annotations

from pathlib import Path
import sys

scripts_dir = Path(__file__).resolve().parent
sys.path.insert(0, str(scripts_dir))

from _impl import (
    aggregate_benchmark,
    analyze_requirements,
    eval_set_editor,
    generate_report,
    generate_scenario_templates,
    improve_description,
    init_skill,
    package_skill,
    quick_validate,
    review,
    run_eval,
    run_loop,
)

COMMANDS = {
    "analyze": {
        "main": analyze_requirements.main,
        "description": "Interactive requirements analysis and design doc generation",
        "aliases": ["analyze-requirements"],
    },
    "init": {
        "main": init_skill.main,
        "description": "Initialize a new skill skeleton",
        "aliases": ["init-skill"],
    },
    "generate-templates": {
        "main": generate_scenario_templates.main,
        "description": "Generate scenario input template drafts from SKILL.md workflows",
        "aliases": ["gen-templates", "templates"],
    },
    "validate": {
        "main": quick_validate.main,
        "description": "Run static SKILL.md validation",
        "aliases": ["quick-validate"],
    },
    "package": {
        "main": package_skill.main,
        "description": "Validate and package a skill",
        "aliases": [],
    },
    "eval": {
        "main": run_eval.main,
        "description": "Run trigger evaluation against an eval-set",
        "aliases": ["run-eval"],
    },
    "loop": {
        "main": run_loop.main,
        "description": "Run eval/improve iteration loop",
        "aliases": ["run-loop"],
    },
    "benchmark": {
        "main": aggregate_benchmark.main,
        "description": "Aggregate benchmark runs into summary stats",
        "aliases": ["aggregate-benchmark"],
    },
    "report": {
        "main": generate_report.main,
        "description": "Generate HTML report from run_loop JSON output",
        "aliases": ["generate-report"],
    },
    "improve": {
        "main": improve_description.main,
        "description": "Generate an improved skill description from eval results",
        "aliases": ["improve-description"],
    },
    "review": {
        "main": review.main,
        "description": "Generate or serve the eval review UI",
        "aliases": ["generate-review"],
    },
    "editor": {
        "main": eval_set_editor.main,
        "description": "Preview or export the eval JSON editor page",
        "aliases": ["eval-editor"],
    },
}

ALIAS_TO_COMMAND = {alias: name for name, config in COMMANDS.items() for alias in config["aliases"]}


def print_help() -> int:
    print("Usage: python scripts/skill_cli.py <command> [args...]\n")
    print("Commands:")
    for name, config in COMMANDS.items():
        alias_text = ""
        if config["aliases"]:
            alias_text = f" (aliases: {', '.join(config['aliases'])})"
        print(f"  {name:<10} {config['description']}{alias_text}")
    print("\nUse 'python scripts/skill_cli.py <command> --help' for command-specific options.")
    return 0


def resolve_command(name: str):
    canonical = ALIAS_TO_COMMAND.get(name, name)
    return canonical, COMMANDS.get(canonical)


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    if not argv or argv[0] in {"-h", "--help", "help"}:
        if len(argv) > 1 and argv[0] == "help":
            command_name, command = resolve_command(argv[1])
            if command is None:
                print(f"Unknown command: {argv[1]}", file=sys.stderr)
                return 1
            entry = command["main"]
            if not callable(entry):
                entry = entry()
            return entry(["--help"])
        return print_help()

    command_name, command = resolve_command(argv[0])
    if command is None:
        print(f"Unknown command: {argv[0]}", file=sys.stderr)
        print(
            "Run 'python scripts/skill_cli.py --help' to see available commands.", file=sys.stderr
        )
        return 1

    entry = command["main"]
    return entry(argv[1:])


if __name__ == "__main__":
    raise SystemExit(main())
