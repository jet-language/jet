# Each period end of each thread's task clock in a samples file, from its
# bytes as `od -A n -v -t u2 -w8` lists them: one line a word, four 16-bit
# fields, low first. The file's two header words come first, then whole
# records; a record's head holds its size in words (head included) in the
# low 11 bits of its fourth field and its kind above them (PerfSamples.jet
# perf_samples_head). A CPU-time sample's record (kind 0) of four words or
# more ends in its time, its task and its count word: its sampler's serial
# in the fourth field, its task clock's count at the sample below it
# (PERF_SAMPLES_TIMED). A sampler's end is a clock record (kind 13) whose
# head addresses 2: its serial, then its task clock's count at its end, then
# the time it ended (perf_precise_ended); the most a serial's ends counted
# wins. A throttle record (kind 13, four words, addressing 3) holds a
# serial, the time the kernel stopped or restarted that sampler's samples,
# and 1 for a stop (perf_samples_ring_throttle). The exit's clock record
# (kind 13, eleven words, addressing 0) holds in its eighth word the samples
# the drains took from the rings, which the file must hold every one of,
# and in its eleventh the samples the kernel dropped, finding no room in a
# ring, which must be none (PerfSamples.jet perf_samples_exit). A time is
# from the process's epoch, all ones before it is known.
#
# The kernel takes a sample at each 125 microsecond period end of a thread's
# task clock and forwards its timer from that end by whole periods, so a
# sampler's consecutive samples' counts lie whole periods apart, give or
# take the latency of the interrupt, and the ends between them took none:
# they fell in kernel mode, where the task clock takes none. So did those
# before its first sample and those after its last, to its end's. A count
# is no fixed multiple of the period, since each switch back in restarts the
# timer with what was left of its period, so the ends are counted gap by
# gap. Two samples under half a period apart took one end. While the kernel
# throttles a sampler its count runs on past the thread's CPU time, so a gap
# a restart falls in, or the stretch after the last sample while throttled
# or once restarted, holds at most the ends its time spans, and those that
# took no sample passed while throttled. Prints that every end took exactly
# one sample, fell in kernel mode or passed while throttled, so the ends
# number the samples kept plus those; else what broke that.
#
# Its own controls: `-v lose=N` reads the file as if its Nth sample were
# missing, and `-v copy=N` as if its Nth sample were written twice.
function word(at) {
    return field0[at] + field1[at] * 65536 + field2[at] * 4294967296 + field3[at] * 281474976710656
}

# A time word, -1 for all ones.
function time_at(at) {
    return field3[at] == 65535 ? -1 : word(at)
}

# Keep the sample whose count word is at `last`, its time two words before.
function take(last) {
    serial = field3[last]
    kept++
    if (serial == 0) {
        untaken++
        return
    }
    count = field0[last] + field1[last] * 65536 + field2[last] * 4294967296
    stamp = time_at(last - 2)
    # Each serial's counts, in ascending order, with their times.
    slot = samples[serial]++
    while (slot > 0 && counts[serial, slot - 1] > count) {
        counts[serial, slot] = counts[serial, slot - 1]
        times[serial, slot] = times[serial, slot - 1]
        slot--
    }
    counts[serial, slot] = count
    times[serial, slot] = stamp
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
        if (kind == 13 && size >= 3 && field0[at] == 2 && field1[at] == 0 && field2[at] == 0) {
            serial = word(at + 1)
            count = word(at + 2)
            if (!(serial in ended) || count > ended[serial]) {
                ended[serial] = count
                end_time[serial] = size >= 4 ? time_at(at + 3) : -1
            }
        }
        if (kind == 13 && size == 4 && field0[at] == 3 && field1[at] == 0 && field2[at] == 0) {
            # Each serial's throttle records, in time order.
            serial = word(at + 1)
            stamp = time_at(at + 2)
            slot = throttles[serial]++
            while (slot > 0 && throttle_at[serial, slot - 1] > stamp) {
                throttle_at[serial, slot] = throttle_at[serial, slot - 1]
                throttle_on[serial, slot] = throttle_on[serial, slot - 1]
                slot--
            }
            throttle_at[serial, slot] = stamp
            throttle_on[serial, slot] = word(at + 3)
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
        since = 0
        held = 0
        upto = 0
        for (slot = 0; slot < samples[serial]; slot++) {
            count = counts[serial, slot]
            stamp = times[serial, slot]
            resumed = 0
            while (stamp >= 0 && upto < throttles[serial] && throttle_at[serial, upto] <= stamp) {
                if (throttle_on[serial, upto] == 0) resumed = 1
                held = throttle_on[serial, upto]
                upto++
            }
            gap = int((count - before + period / 2) / period)
            if (gap < 1) {
                twice++
                gap = 1
            }
            if (resumed && since >= 0 && stamp >= since) {
                spans = int((stamp - since + period / 2) / period)
                taken = spans > 1 ? spans : 1
                if (taken > gap) taken = gap
                cut += gap - taken
                throttled += taken - 1
            } else kernel += gap - 1
            reached += gap
            before = count
            since = stamp
        }
        if (samples[serial] > 0 && before > ended[serial]) past++
        if (ends > reached) {
            resumed = 0
            for (; upto < throttles[serial]; upto++) {
                if (throttle_on[serial, upto] == 0) resumed = 1
            }
            tail = ends - reached
            if ((held || resumed) && since >= 0 && end_time[serial] >= since) {
                spans = int((end_time[serial] - since + period / 2) / period)
                taken = spans < tail ? spans : tail
                cut += tail - taken
                throttled += taken
            } else kernel += tail
        }
        nominal += ends > reached ? ends : reached
    }
    nominal -= cut
    for (serial in samples) {
        if (!(serial in ended)) unended++
    }
    lacks = rings > kept ? rings - kept : 0
    beyond = kept > rings ? kept - rings : 0
    if (kept > 0 && lacks == 0 && beyond == 0 && dropped == 0 && untaken == 0 && twice == 0 && past == 0 && unended == 0 && nominal == kept + kernel + throttled) {
        print "every period end took one sample, fell in kernel mode or passed while throttled"
    } else {
        print "period ends " nominal + 0 ", samples " kept + 0 ", in kernel mode " kernel + 0 ", passed while throttled " throttled + 0 ", samples the rings took that the file lacks " lacks ", samples beyond those the rings took " beyond ", samples the kernel dropped " dropped + 0 ", samples no sampler took " untaken + 0 ", period ends that took two " twice + 0 ", samplers with samples past their end " past + 0 ", samplers with no end " unended + 0
    }
}
