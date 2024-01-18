# Why would I want to contribute?

This project is intended to be developed and maintained by a commercial company as a component with no goal of becoming commercial in itself.
It is more of a public utility that we hope will be useful to many people and companies.

# I’m in! Now what?

[Join the Discord Server!](https://discord.gg/XMetBW3zCG).

## Principles

- **Less prompt is more**. The API user should have as much control over the prompts and we should hard code as little as possible.
- **Follow strictly OpenAI API**. We should rarely deviate or add to the API. We should only do so when it is clear that the API is lacking.
- Rely on **open source projects that survived the test of time**.
- Rely on **cloud native** technologies that survived the test of time.
- **Standalone and edge ready**. The API should be able to run on a Raspberry Pi for example.
- **Test as much as possible**. We should have a high test coverage.

## Current Stack

### Infra

* Postgres for all the cold data (retrieval, functions, assistants, etc.)
* Redis for all the hot data (atm only queuing runs, next could be used for [caching](https://github.com/stellar-amenities/assistants/issues/51))
* Minio for all the files
* Docker

### Backend

* Rust, Axum, etc.

### Taking on Tasks

We have a growing task list of
[issues](https://github.com/stellar-amenities/assistants/issues). Find an issue that
appeals to you and make a comment that you'd like to work on it. Include in your
comment a brief description of how you'll solve the problem and if there are any
open questions you want to discuss.

If the issue is currently unclear but you are interested, please post in Discord
and someone can help clarify the issue in more detail.

### Setup

1. Install Rust `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. [Install Docker](https://docs.docker.com/engine/install/)
3. Run docker services postgres, redis, and minio `docker-compose -f docker/docker-compose.yml up postgres redis minio`
4. Run the server and runs executor: `make all` (it's running `cargo` under the hood which is the package manager of Rust)

"runs executor" is just a piece of code that wait items being added in Redis and execute runs when something is added.

We recommend using [Cursor](https://cursor.sh/) for programming - best IDE nowadays. Also make sure to install Rust extension.

### Submitting Work

We're all working on different parts of Assistants together. To make
contributions smoothly we recommend the following:

1.  [Fork this project repository](https://docs.github.com/en/get-started/quickstart/fork-a-repo)
    and clone it to your local machine. (Read more
    [About Forks](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/working-with-forks/about-forks)) or use gitpod.io.
1.  Before working on any changes, try to
    [sync the forked repository](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/working-with-forks/syncing-a-fork)
    to keep it up-to-date with the upstream repository.
1.  On a
    [new branch](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/proposing-changes-to-your-work-with-pull-requests/creating-and-deleting-branches-within-your-repository)
    in your fork (aka a "feature branch" and not `main`) work on a small focused
    change that only touches on a few files.
1.  Package up a small bit of work that solves part of the problem
    [into a Pull Request](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/proposing-changes-to-your-work-with-pull-requests/creating-a-pull-request-from-a-fork)
    and
    [send it out for review](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/proposing-changes-to-your-work-with-pull-requests/requesting-a-pull-request-review).
1.  If you're lucky, we can merge your change into `main` without any problems.
    If there are changes to files you're working on, resolve them by:
    1.  First try to rebase as suggested
        [in these instructions](https://timwise.co.uk/2019/10/14/merge-vs-rebase/#should-you-rebase).
    1.  If rebasing feels too painful, merge as suggested
        [in these instructions](https://timwise.co.uk/2019/10/14/merge-vs-rebase/#should-you-merge).
1.  Once you've resolved conflicts (if any), finish the review and
    [squash and merge](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/incorporating-changes-from-a-pull-request/about-pull-request-merges#squash-and-merge-your-commits)
    your PR (when squashing try to clean up or update the individual commit
    messages to be one sensible single one).
1.  Merge in your change and move on to a new issue or the second step of your
    current issue.

Additionally, if someone is working on an issue that interests you, ask if they
need help on it or would like suggestions on how to approach the issue. If so,
share wildly. If they seem to have a good handle on it, let them work on their
solution until a challenge comes up.

### Releasing new Docker images

Just commit with Release and version number in the commit message. For example:

`git commit -m "Release 1.0.0"`

#### Tips

- At any point you can compare your feature branch to the upstream/main of
  `stellar-amenities/assistants` by using a URL like this:
  https://github.com/stellar-amenities/assistants/compare/main...bobm4894:assistants:my-example-feature-branch.
  Obviously just replace `bobm4894` with your own GitHub user name and
  `my-example-feature-branch` with whatever you called the feature branch you
  are working on, so something like
  `https://github.com/stellar-amenities/assistants/compare/main...<your_github_username>:assistants:<your_branch_name>`.
  This will show the changes that would appear in a PR, so you can check this to
  make sure only the files you have changed or added will be part of the PR.
- Try not to work on the `main` branch in your fork - ideally you can keep this
  as just an updated copy of `main` from `stellar-amenities/assistants`.
- If your feature branch gets messed up, just update the `main` branch in your
  fork and create a fresh new clean "feature branch" where you can add your
  changes one by one in separate commits or all as a single commit.
- When working on Github actions, you can test locally using [act](https://github.com/nektos/act) like so `act -W .github/workflows/ci_core.yml --container-architecture linux/amd64` (container-architecture is necessary if you use Mac M series)

### When does a review finish

A review finishes when all blocking comments are addressed and at least one
owning reviewer has approved the PR. Be sure to acknowledge any non-blocking
comments either by making the requested change, explaining why it's not being
addressed now, or filing an issue to handle it later.


### Troubleshooting

- Sometimes you need to run `cargo sqlx prepare --workspace` - typically upon writing sql queries or editing the migrations. SQLX runs checks at build time, welcome to Rust!

