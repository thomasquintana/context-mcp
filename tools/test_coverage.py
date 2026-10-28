"""Independent regression-gate checks; percentages cannot hide another crate."""
import unittest
from coverage import PACKAGES,check

class GateTests(unittest.TestCase):
    def test_every_crate_has_independent_floor_and_uncovered_budget(self):
        values={name:{"lines":100,"covered":90,"uncovered":10,"percent":90} for name in PACKAGES}
        limits={name:{"minimum_percent":89,"maximum_uncovered":11} for name in PACKAGES}
        self.assertEqual(check(values,limits),[])
        values["context-engine"]["uncovered"]=12
        self.assertIn("context-engine: uncovered-line budget exceeded",check(values,limits))
        values["context-auth"]["percent"]=88
        self.assertIn("context-auth: line floor regressed",check(values,limits))
        values["context-mcp"]["lines"]=0
        self.assertIn("context-mcp: missing measured production code/budget",check(values,limits))

if __name__=="__main__":unittest.main()
