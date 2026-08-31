use jet::CLI;

#[test]
fn c_drivers_are_registered_with_one_flag_surface() {
    for (command, suffix) in [("cc", "cc [options] <sources>"), ("c++", "c++ [options] <sources>")] {
        assert!(CLI::is_builtin(command));
        assert!(CLI::owns_flag_vocabulary(command));
        assert!(CLI::command_usage(command).ends_with(suffix));

        let flags = CLI::flags_for_command(command);
        for flag in [
            "--offline",
            "--project-root",
            "--build-root",
            "-c",
            "-o",
            "-MMD",
            "-MD",
            "-MF",
            "-MT",
            "--sysroot",
            "-I",
            "-D",
            "-L",
            "-l",
            "-std",
            "--target",
            "-dumpmachine",
            "-print-sysroot",
            "-dumpversion",
            "-v",
        ] {
            assert!(
                flags.iter().any(|(name, _)| *name == flag),
                "{command} is missing {flag}"
            );
        }
    }
}
