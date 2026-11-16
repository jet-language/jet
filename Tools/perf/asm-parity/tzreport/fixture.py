#!/usr/bin/env python3
# The tzreport cell's input: the gauntlet entry's checked-in meetings file,
# copied to argv[1].
import shutil
import sys
from pathlib import Path

shutil.copyfile(Path(__file__).resolve().parents[3] / "gauntlet/entries/tzreport/meetings.txt", sys.argv[1])
