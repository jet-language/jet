import assert from 'node:assert/strict';
import test from 'node:test';
import {trapLiteral, schedulerLiteral} from './report-literals.mjs';

test('process-edge literals preserve exact bytes, including signed UTF-8 words', () => {
    assert.equal(trapLiteral('trap_msg_prelude_end').toString('utf8'), '`\n  --> <core.prelude>:0\n');
    assert.equal(trapLiteral('trap_msg_todo_expected').toString('utf8'), ' — expected ');
    assert.equal(schedulerLiteral('scheduler_msg_cancelled').toString('utf8'), 'task cancelled at a cooperative wait point');
    assert.equal(schedulerLiteral('scheduler_msg_deadline_tail').toString('utf8'), '\n Why: This wait point observed the task context deadline from `#Context(deadline: …)`\n Fix: Raise the deadline budget or shorten the work before this wait point\nMore: jet-lang.dev/e/E3003');
});

test('a missing generated report literal cannot become an empty golden', () => {
    assert.throws(() => trapLiteral('missing_report_literal'), /missing frozen trap literal/);
    assert.throws(() => schedulerLiteral('missing_report_literal'), /missing frozen scheduler literal/);
});
