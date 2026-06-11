// 11 — Enums and exhaustive switch (M3, S30/S31).

enum Light {
    Red;
    Yellow;
    Green;
}

fn next(light: Light) -> Light {
    switch light {
        light == Red -> { return Light.Yellow; };
        light == Yellow -> { return Light.Green; };
        light == Green -> { return Light.Red; };
    }
}

fn label(light: Light) -> String {
    switch light {
        light == Red -> { return "stop"; };
        light == Yellow -> { return "caution"; };
        light == Green -> { return "go"; };
    }
}

fn main() {
    val start = Light.Red;
    print(label(start));
    print(label(next(start)));
}
