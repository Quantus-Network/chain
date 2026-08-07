# Security Policy

Quantus Network takes the security of our users seriously and welcomes reports
from security researchers.

## Reporting a vulnerability

**Please do not open a public issue, pull request, or social-media post for a
security vulnerability.** Public disclosure before a fix is available puts
users' funds at risk. Use one of the private channels below and we will
coordinate a fix and disclosure with you.

1. **GitHub private security advisory (preferred).**
   [Report a vulnerability](https://github.com/quantus-network/chain/security/advisories/new).
   This gives you a private, structured thread with the maintainers and is the
   fastest way to reach us.
2. **Email.** Send details to **security@quantus.com**.

### What to include

- A clear description of the issue and its security impact.
- A working proof of concept is required. It must run directly against this
  project, using a release build or locally built version, and demonstrate the
  reported behavior and security impact. Standalone code, mathematical
  examples, or simulations that only reproduce the theory without exercising
  the project do not satisfy this requirement.
- Step-by-step reproduction instructions.
- Affected platforms and versions.
- Any relevant logs, addresses, or transaction IDs (for on-chain issues).

If you used AI tooling to find or write up the report, please say so.

## Our commitment (safe harbor)

We consider security research conducted in good faith under this policy to be
authorized. We will not pursue or support legal action against researchers who:

- make a good-faith effort to avoid privacy violations, data destruction, and
  interruption or degradation of our services;
- only interact with accounts they own or have explicit permission to access;
  and
- give us a reasonable opportunity to fix an issue before disclosing it
  publicly.

If in doubt about whether an action is authorized, ask us first at
security@quantus.com.

## What to expect

- **Acknowledgement:** within **2 business days**.
- **Triage and initial assessment:** within **7 business days**.
- **Coordinated disclosure:** we aim to ship a fix and coordinate public
  disclosure within **90 days** of the report. We will keep you updated on
  progress and agree on a disclosure date with you.
- **Credit:** with your permission, we are happy to publicly credit you for the
  report once a fix is released.

## Scope

**In scope:** the Quantus Network node, runtime, and QPoW consensus in this repository — anything that could lead to loss of funds, a chain split or consensus failure, remote crash or denial of service of nodes, or exposure of keys in the node keystore.

**Out of scope:** issues in third-party services or nodes we do not operate;
reports generated solely by automated scanners without a demonstrated impact;
low-severity or informational issues on our marketing and landing websites;
and social-engineering or physical attacks.

## Rewards

At our **sole discretion**, we may offer a reward for a valid report. To be
eligible, a report must be submitted **privately** through one of the channels
above and identify a genuine vulnerability with real impact on users. Trivial
or low-impact findings, automated-scanner output without a working proof of
concept, and already-known issues are **not** eligible. There is no fixed
bounty and no guaranteed payout; whether a report qualifies, and any amount,
are determined solely by Quantus Network.

## Supported versions

Only the latest release is supported; security fixes are delivered in new
versions. Please keep your installation up to date.
