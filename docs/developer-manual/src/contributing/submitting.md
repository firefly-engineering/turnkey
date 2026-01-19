# Submitting Changes

## Before You Start

1. Check existing issues for related work
2. For large changes, open an issue first to discuss approach
3. Fork the repository

## Development Workflow

1. Create a feature branch:
   ```bash
   git checkout -b feat/my-feature
   ```

2. Make your changes

3. Test thoroughly:
   ```bash
   tk test //...
   nix flake check
   ```

4. Commit with conventional message:
   ```bash
   git commit -m "feat: add zig toolchain support"
   ```

## Pull Request Process

1. Push your branch:
   ```bash
   git push -u origin feat/my-feature
   ```

2. Open a Pull Request on GitHub

3. Fill out the PR template:
   - Summary of changes
   - Test plan
   - Related issues

4. Wait for review

## PR Checklist

- [ ] Tests pass (`tk test //...`)
- [ ] Nix flake checks pass (`nix flake check`)
- [ ] Pre-commit hooks pass
- [ ] Documentation updated if needed
- [ ] Commit messages follow convention

## Review Process

- Maintainers will review within a few days
- Address feedback with additional commits
- Once approved, maintainer will merge

## After Merge

- Delete your feature branch
- Pull latest main
- Thanks for contributing!

## Getting Help

- Open an issue for questions
- Tag maintainers if stuck on review
- Check existing PRs for examples
