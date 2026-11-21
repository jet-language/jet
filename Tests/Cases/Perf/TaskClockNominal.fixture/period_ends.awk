# Each period end of each thread's task clock in a samples file, from its
# bytes as `od -A n -v -t u2 -w8` lists them: one line a word, four 16-bit
# fields, low first. The file's two header words come first, then whole
# records; a record's head holds its size in words (head included) in the
# low 11 bits of its fourth field and its kind above them (PerfSamples.jet
# perf_samples_head). A CPU-time sample's record (kind 0) of four words or
# more ends in its count word: its sampler's serial in the fourth field, its
# task clock's count at the sample below it (PERF_SAMPLES_TIMED). A
# sampler's end is a clock record (kind 13) of three words whose head
# addresses 2: its serial, then its task clock's count at its end
# (perf_precise_ended); the most a serial's ends counted wins. The exit's
# clock record (kind 13, eleven words, addressing 0) holds in its eighth
# word the samples the drains took from the rings, which the file must hold
# every one of, and in its eleventh the samples the kernel dropped, finding
# no room in a ring, which must be none (PerfSamples.jet perf_samples_exit).
#
# The kernel takes a sample at each 125 microsecond period end of a thread's
# task clock and forwards its timer from that end by whole periods, so a
# sampler's consecutive samples' counts lie whole periods apart, give or
# take the latency of the interrupt, and the ends between them took none:
# they fell in kernel mode, where the task clock takes none. So did those
# before its first sample and those after its last, to its end's. A count
# is no fixed multiple of the period, since each switch back in restarts the
# timer with what was left of its period, so the ends are counted gap by
# gap. Two samples under half a period apart took one end. Prints that
# every end took exactly one sample or fell in kernel mode, so the ends
# number the samples kept plus those that fell in kernel mode; else what
# broke that.
#
# Its own controls: `-v lose=N` reads the file as if its Nth sample were
# missing, and `-v copy=N` as if its Nth sample were written twice.
function word(at) {
    return field0[at] + field1[at] * 65536 + field2[at] * 4294967296 + field3[at] * 281474976710656
}

# Keep the sample whose count word is at `last`.
function take(last) {
    serial = field3[last]
    kept++
    if (serial == 0) {
        untaken++
        return
    }
    count = field0[last] + field1[last] * 65536 + field2[last] * 4294967296
    # Each serial's counts, in ascending order.
    slot = samples[serial]++
    while (slot > 0 && counts[serial, slot - 1] > count) {
        counts[serial, slot] = counts[serial, slot - 1]
        slot--
    }
    counts[serial, slot] = count
}

{
    field0[words] = $1
    field1[words] = $2
    field2[words] = $3
    field3[words] = $4
    words++
}

END {
    period = 125000
    at = 2
    while (at < words) {
        size = field3[at] % 2048
        kind = int(field3[at] / 2048)
        if (size == 0 || at + size > words) {
            print "a record at word " at " runs past the file's " words " words"
            exit
        }
        if (kind == 0 && size >= 4) {
            sample++
            if (sample != lose) take(at + size - 1)
            if (sample == copy) take(at + size - 1)
        }
        if (kind == 13 && size == 3 && field0[at] == 2 && field1[at] == 0 && field2[at] == 0) {
            serial = word(at + 1)
            count = word(at + 2)
            if (!(serial in ended) || count > ended[serial]) ended[serial] = count
        }
        if (kind == 13 && size == 11 && field0[at] == 0 && field1[at] == 0 && field2[at] == 0) {
            if (word(at + 7) > rings) rings = word(at + 7)
            if (word(at + 10) > dropped) dropped = word(at + 10)
        }
        at += size
    }
    for (serial in ended) {
        ends = int(ended[serial] / period)
        reached = 0
        before = 0
        for (slot = 0; slot < samples[serial]; slot++) {
            count = counts[serial, slot]
            gap = int((count - before + period / 2) / period)
            if (gap < 1) {
                twice++
                gap = 1
            }
            kernel += gap - 1
            reached += gap
            before = count
        }
        if (samples[serial] > 0 && before > ended[serial]) past++
        if (ends > reached) kernel += ends - reached
        nominal += ends > reached ? ends : reached
    }
    for (serial in samples) {
        if (!(serial in ended)) unended++
    }
    lacks = rings > kept ? rings - kept : 0
    beyond = kept > rings ? kept - rings : 0
    if (kept > 0 && lacks == 0 && beyond == 0 && dropped == 0 && untaken == 0 && twice == 0 && past == 0 && unended == 0 && nominal == kept + kernel) {
        print "every period end took one sample or fell in kernel mode"
    } else {
        print "period ends " nominal + 0 ", samples " kept + 0 ", in kernel mode " kernel + 0 ", samples the rings took that the file lacks " lacks ", samples beyond those the rings took " beyond ", samples the kernel dropped " dropped + 0 ", samples no sampler took " untaken + 0 ", period ends that took two " twice + 0 ", samplers with samples past their end " past + 0 ", samplers with no end " unended + 0
    }
}
