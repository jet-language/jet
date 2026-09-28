# CoreLib-Parallel Tower Card Handoff

**Board snapshot:** 2026-09-21  
**Source of truth:** Tower remains authoritative. This file is a handoff snapshot, not a second work ledger.

## Scope and classification

The board contains **591 active non-bug cards**.

A card is in the parallel handoff pool only when it is:

- not a bug;
- not in JetOS epoch `e9`;
- not tagged `corelib`;
- not linked to `CoreModuleExports`, `core_calls`, `Prelude/CoreLib`, `Core/`, `Core.jet`, or Core generators;
- not an owner decision;
- not already assigned to Main;
- not the owner-deferred self-hosting gate `#217`.

The result is:

| Bucket | Count | Action |
|---|---:|---|
| Unclaimed, Core-independent handoff pool | 307 | Safe to hand off in parallel |
| Immediately actionable subset | 218 | No `blockedBy` entry and no `needsAcceptance` gate |
| Core-independent but gated | 89 | Prepare in parallel; execute after independent blockers |
| Core-coupled cards | 153 | Hold for or coordinate with CoreLib conversion |
| JetOS `e9` cards | 110 | Excluded by standing scope |
| Non-Core owner decisions | 16 | Agent may prepare; owner must ratify |
| Already assigned to Main | 4 | Do not hand off |
| Owner-deferred self-hosting gate | 1 | Do not start |

Even the immediately actionable cards cannot claim compiler/runtime closure while generated Core views are stale. They can be researched, planned, implemented, and tested with source-local fixtures. Integrated compiler proofs wait for a fresh Core cutover.

## Wave A: immediately actionable, 218 cards

These cards have no `blockedBy` entries and no `needsAcceptance` gate.

### Unscoped sidequests — 35

```text
#2257 #2279 #2287 #2414 #2979 #2997 #2998 #2999 #3000 #3001
#3002 #3003 #3008 #3011 #3014 #3016 #3021 #3025 #3037 #3399
#3400 #3401 #3402 #3403 #3404 #3407 #3418 #3420 #3501 #3504
#3505 #3506 #3507 #3508 #3509
```

### E10 adoption/reference learning — 5

```text
#3120 #3162 #3173 #3177 #3195
```

### E11 portable targets — 1

```text
#758
```

### E11 MCP interoperability — 1

```text
#768
```

### E11 ecosystem adoption — 3

```text
#507 #1344 #2199
```

### E12 bounded services — 2

```text
#3123 #3305
```

### E13 platform cards — 3

```text
#524 #525 #527
```

The excluded cross-platform bug cards `#467`, `#961`, and `#962` are not in this non-bug list.

### E14 devtools, live loop, toolchain, and proof — 18

```text
#3116 #3168 #3187 #3188 #3191 #3198
#3091 #3113 #3196
#3172
#3174 #3189 #3192 #3197 #3203 #3204 #3205 #3369
```

### E15 backend — 3

```text
#3092 #3096 #3163
```

### E3 language and surface work

Language and surface — 4:

```text
#1814
#1401
#1427 #1436
```

Web target — 1:

```text
#1914
```

Enums and reflection — 24:

```text
#3131 #3132 #3133 #3134 #3136 #3137 #3138 #3139 #3140 #3142
#3143 #3144 #3145 #3146 #3147 #3152 #3154 #3156 #3157 #3159
#3160 #3161 #3167 #3194
```

Iterators and stdlib — 50:

```text
#3106 #3313 #3322 #3324 #3329 #3331 #3332 #3333 #3338 #3340
#3341 #3342 #3343 #3345 #3348 #3349 #3350 #3352 #3353 #3354
#3355 #3356 #3357 #3360 #3361 #3362 #3363 #3365 #3366 #3367
#3368 #3371 #3372 #3373 #3374 #3375 #3376 #3377 #3378 #3384
#3385 #3386 #3387 #3388 #3389 #3390 #3391 #3392 #3394 #3395
```

Encoding and validation — 8:

```text
#3107 #3124 #3125 #3148 #3149 #3150 #3151 #3165
```

CLI and text — 2:

```text
#3121 #3122
```

Checked source facts — 7:

```text
#3079 #3158 #3175 #3181 #3193 #3200 #3321
```

HTTP and service runtime — 6:

```text
#3086 #3090 #3094 #3095 #3126 #3303
```

Runtime and concurrency — 4:

```text
#3104 #3105 #3108 #3180
```

Tier parity — 3:

```text
#1218 #1221 #1764
```

Core-surface follow-up cards without direct Core write references — 2:

```text
#1464 #1471
```

### E4 compiler facade and build facts — 6

```text
#3153 #3164 #3166 #3199 #3201 #3202
```

### E5 registry, trust, and environment — 10

```text
#3112
#3089 #3119 #3127 #3220 #3380 #3381 #3382 #3383
#3103
```

### E6 plugin and accelerator — 16

```text
#3073 #3102 #3109 #3110 #3111 #3114 #3115
#3135 #3141 #3210 #3211 #3218 #3219 #3221 #3222 #3223
```

### E7 evidence — 2

```text
#3118 #3117
```

## Wave B: parallel preparation, 89 gated cards

These are independent of CoreLib but blocked by their own prerequisites or owner acceptance.

### Unscoped gates — 25

```text
#2268 #2393 #2394 #3004 #3012 #3023 #3029 #3030 #3031 #3032
#3033 #3036 #3040 #3042 #3408 #3409 #3410 #3411 #3412 #3413
#3414 #3419 #3422 #3423 #3503
```

`#3030`, `#3031`, `#3032`, `#3414`, and `#3419` require owner acceptance. An agent may prepare the implementation or ballot but cannot close the owner gate.

### E11 portable targets — 4

```text
#1058 #1059 #1060 #1227
```

### E11 compiler and build loop — 18

```text
#2514 #2517 #2519 #2520 #2521 #2522 #2523 #2524 #2525 #2526
#2527 #2528 #2529 #2530 #2531 #2532 #2533 #2534
```

These form an internal dependency chain. Use disjoint path ownership.

### E11 MCP interoperability — 8

```text
#1061 #1062 #1063 #1064 #1065 #1066 #1067 #1068
```

### E11 ecosystem adoption — 7

```text
#1345 #1346 #1347 #1348 #1349 #2200 #2201
```

### E11 workload proof — 1

```text
#1414
```

### E11 deferred self-hosting — 14

```text
#218 #668 #669 #670 #671 #808 #809 #810 #811 #812 #813 #814 #815 #816
```

These remain blocked by the owner-deferred `#217` gate. Do not execute until the owner reopens that chain.

### E12 security closure — 1

```text
#1387
```

### E13 platform proof — 2

```text
#1228 #1229
```

### E14 mobile — 2

```text
#2443 #2444
```

### E3 bucket — 1

```text
#1804
```

### E3 HTTP integrations — 4

```text
#3081 #3082 #3083 #3084
```

### E5 registry — 1

```text
#3098
```

### E5 platform acceptance — 1

```text
#956
```

## Hold: Core-coupled cards, 153 cards

Do not hand these off during the CoreLib conversion.

### Current Core conversion

```text
#3515 #3516
```

### Unscoped Core-adjacent work

```text
#2994 #2995 #3005 #3006 #3007 #3018 #3022 #3027 #3034 #3035
#3421 #3447 #3449
```

### Core HTTP, game, platform, data, and GUI surfaces

```text
#17
#238 #820 #822 #823 #824 #825
#3077 #3263
#3224 #3227
#523 #526
#996 #998 #1182
#3250
#3287
#3228
#3254 #3308
#3243 #3257 #3258 #3259 #3260
#3244
#3155 #3232
```

### Core iterator surface

```text
#3041 #3231 #3281 #3315 #3316 #3317 #3318 #3319 #3320 #3325
#3326 #3327 #3328 #3337 #3339 #3347 #3358 #3359 #3364 #3370
```

### Core encoding and validation surface

```text
#3233 #3234 #3236 #3237 #3238 #3255 #3262 #3267 #3268 #3269
#3271 #3272 #3273 #3274 #3275 #3276 #3277 #3278 #3279 #3280
#3282 #3284 #3285 #3286 #3295 #3296 #3297 #3298 #3299 #3300
#3301 #3306 #3312
```

### Core CLI and text surface

```text
#3241 #3242 #3251 #3252 #3283 #3289 #3290 #3294 #3307 #3311
```

### Core checked facts, services, and runtime

```text
#3176 #3182 #3186
#3071 #3075 #3076 #3078 #3080 #3085 #3087 #3088 #3225 #3229 #3302 #3304
#3179 #3226 #3230 #3239 #3240 #3245 #3248 #3261 #3265
#3264 #3288
#3266 #3310
#3246 #3270
#3072 #3253
#3026 #3028 #3206 #3207 #3208 #3209
#3249
#3235 #3247
#999 #1000
#3093 #3256 #3309
```

## Other exclusions

### JetOS

All active non-bug cards in epoch `e9`: **110 cards**. They remain excluded by the standing scope.

### Non-Core owner decisions

```text
#3212 #3213 #3214 #3215 #3216 #3217
#3330 #3334 #3335 #3336 #3344 #3346 #3351 #3379 #3393 #3452
```

Agents may prepare proposals and evidence. The owner must ratify these decisions.

### Already assigned to Main

```text
#2386 #2858 #2919 #3067
```

### Owner-deferred self-hosting gate

```text
#217 Bootstrap readiness gate
```

## Recommended dispatch waves

1. **Research and planning:** unscoped planning cards plus E10 and E14 evidence cards.
2. **Language:** enum/reflection and non-Core iterator cards. Avoid Core collection files.
3. **Tooling:** LSP, devtools, live-loop, compiler-facade, and evidence cards.
4. **Runtime:** non-Core service, concurrency, backend, and plugin cards.
5. **Build:** E11 compiler-loop cards, respecting their dependency chain.
6. **Platform:** E11 and E13 target and packaging cards, excluding `#467`, `#961`, and `#962`.

Use one implementer per coherent path. Do not put all cards in one worktree. Tower remains the work ledger, and no card is complete until its focused proof and criteria evidence are recorded there.
