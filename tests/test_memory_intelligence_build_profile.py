"""Keep the opt-in producer's documented build profile executable."""

from pathlib import Path
import shlex
import unittest


ROOT = Path(__file__).resolve().parents[1]


class ProducerBuildProfileTests(unittest.TestCase):
    def test_documented_producer_build_explicitly_enables_llm(self):
        readme = (ROOT / "benches/memory_intelligence/README.md").read_text(
            encoding="utf-8"
        )
        section = readme.split("## Selected-runtime producer\n", 1)[1]
        example = section.split("```bash\n", 1)[1].split("```", 1)[0]
        commands = [shlex.split(line) for line in example.splitlines() if line.strip()]
        build = next(command for command in commands if command[:2] == ["cargo", "build"])
        features = []
        for index, argument in enumerate(build):
            if argument == "--features":
                self.assertLess(index + 1, len(build), "--features needs a value")
                features.extend(build[index + 1].replace(",", " ").split())
            elif argument.startswith("--features="):
                features.extend(argument.split("=", 1)[1].replace(",", " ").split())
        self.assertIn(
            "llm",
            features,
            "The evaluation command is feature-gated; its documented build must explicitly enable llm",
        )


if __name__ == "__main__":
    unittest.main()
