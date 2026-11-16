#!/usr/bin/env bash
# The stand-in precommit of the signal cases: an operator's signal ($LANDER_SIGNAL) reaches the lander
# while its patch is applied and this precommit runs, which outlives the rollback.
kill -s "$LANDER_SIGNAL" "$PPID"
sleep 5
