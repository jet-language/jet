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
# (perf_precise_ended); the most a serial's ends counted wins.
#
# The kernel takes a sample at each 125 microsecond period end of a thread's
# task clock, which it forwards by whole periods, so a sample's count
# rounds to the end that took it. An end before a sampler's end that no
# sample rounds to fell in kernel mode, where the task clock takes none:
# where consecutive samples' counts are two or more periods apart, before
# the first and after the last. Prints that every end took exactly one
# sample or fell in kernel mode, so the ends number the samples kept plus
# those that fell in kernel mode; else what broke that.
function word(at) {
    return field0[at] + field1[at] * 65536 + field2[at] * 4294967296 + field3[at] * 281474976710656
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
            last = at + size - 1
            serial = field3[last]
            kept++
            if (serial == 0) {
                untaken++
            } else {
                end = int((field0[last] + field1[last] * 65536 + field2[last] * 4294967296 + period / 2) / period)
                if ((serial, end) in took) twice++
                took[serial, end] = 1
                if (!(serial in most) || end > most[serial]) most[serial] = end
            }
        }
        if (kind == 13 && size == 3 && field0[at] == 2 && field1[at] == 0 && field2[at] == 0) {
            serial = word(at + 1)
            count = word(at + 2)
            if (!(serial in ended) || count > ended[serial]) ended[serial] = count
        }
        at += size
    }
    for (serial in ended) {
        ends = int(ended[serial] / period)
        nominal += ends
        for (end = 1; end <= ends; end++) {
            if (!((serial, end) in took)) kernel++
        }
        if (serial in most && most[serial] > ends) past++
    }
    for (serial in most) {
        if (!(serial in ended)) unended++
    }
    if (kept > 0 && untaken == 0 && twice == 0 && past == 0 && unended == 0 && nominal == kept + kernel) {
        print "every period end took one sample or fell in kernel mode"
    } else {
        print "period ends " nominal + 0 ", samples " kept + 0 ", in kernel mode " kernel + 0 ", samples no sampler took " untaken + 0 ", period ends that took two " twice + 0 ", samplers with samples past their end " past + 0 ", samplers with no end " unended + 0
    }
}
