#!/usr/bin/env python3
# The tzreport cell's input: the gauntlet entry's checked-in meetings file,
# copied to argv[1].
import shutil
import sys
from pathlib import Path

shutil.copyfile(Path(__file__).resolve().parents[3] / "Gauntlet/Entries/Tzreport/meetings.txt", sys.argv[1])
