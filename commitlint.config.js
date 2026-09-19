export default {
  extends: ["@commitlint/config-conventional"],
  // Dependabot writes release notes and compare links into the body, well past
  // the 72-column limit, and that is the update path the cooldown protects.
  ignores: [(message) => message.includes("Signed-off-by: dependabot[bot]")],
  rules: {
    "scope-enum": [
      2,
      "always",
      // The mod names, the directories no crate owns, and the cross-cutting
      // names. `cargo xtask scopes` prints the same list.
      [
        "private-chests",
        "common",
        "xtask",
        "fixtures",
        "deps",
        "ci",
        "release",
      ],
    ],
    "body-max-line-length": [2, "always", 72],
  },
};
