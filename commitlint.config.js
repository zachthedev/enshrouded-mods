export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'scope-enum': [2, 'always', ['private-chests', 'common', 'xtask', 'fixtures', 'deps', 'ci', 'release']],
    'body-max-line-length': [2, 'always', 72],
  },
};
