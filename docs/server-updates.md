# Updating the server from the admin panel

The owner can update a Cereal server from **Instance settings → Updates**: it shows the version that runs,
whether a newer one exists, an **Update server** button with live progress, and how the last update ended.

This needs one thing installed on the machine that runs the server: the updater. Without it the Updates
section says so and offers nothing to press; `scripts/deploy-ssh.sh` keeps working either way.

## How it works

```
 browser ──HTTPS──▶ server container ──request file──▶ <deployment>-releases/.updater/spool ◀── scripts/updater.sh
 (owner)            no Docker socket   ◀─state, log──    (a folder mounted into the container)    on the host: git, docker
```

- **The server's container never gets the Docker socket.** Updating means building an image and restarting
  containers; the program that does it, `scripts/updater.sh`, runs on the host as the user who runs
  `docker compose` there.
- **The server can ask for two things only:** "look for a newer version" and "update to it". It does so by
  leaving a small file in `spool/requests`. The file carries a SHA-256 over a shared secret
  (`UPDATER_TOKEN`), a rising timestamp and the action, so a file from anyone else on the machine, or a copy
  of an earlier request, is thrown away. Where a version comes from is written on the host and cannot be
  named in a request.
- **Progress comes back as files** the server only reads: `state` (what runs, what was found), `job` (the
  update under way or the last one) and `log`. They are outside the containers, so they survive the restart;
  the page reconnects to the new server and shows how it ended.
- **An update is the procedure of `scripts/deploy-ssh.sh`**, run locally: both drive
  `scripts/release-helper.sh`. Database dump to `~/cereal-backups`, a new release folder with the same `.env`
  and published downloads, image build while the old server answers, switch, migrations, wait for healthy,
  and back to the previous release if any of that fails. The database, audio and settings volumes are the
  same whichever release runs.

## Where a newer version comes from

**The releases of the repository.** A release is a tag named like `v1.2.3`; publishing a release on GitHub
makes one. `./scripts/release-github.sh` publishes the version in `package.json` as a release and attaches
the Mac build (see "Publishing a version" below). The updater takes the release with the highest number and
ignores tags with anything after the number (`v1.3.0-beta.1`). It reads them with git, so it needs a remote
the server can read:

1. **A public repository:** `./scripts/updater.sh install https://github.com/<you>/cereal.git`.
2. **A private repository, with a read-only deploy key.** Add a deploy key (read access only) to the
   repository on GitHub, put the private key on the server, and install with
   `./scripts/updater.sh install git@github.com:<you>/cereal.git --ssh-key ~/.ssh/cereal_deploy_key`.

The latest release is offered when it is ahead of the commit this server runs. A server deployed from a
commit that is in no release (`scripts/deploy-ssh.sh` from a branch) is compared by version number instead:
only a release with a higher number is offered. Nothing behind the running version is ever installed
(migrations only go forward). The version number shown is `version` in the release's `package.json`, so
keep it the same as the tag. The updater looks when the owner presses **Check for updates**, when it
starts, and every six hours.

**Or one branch or tag.** Give `install` a ref and the updater follows that instead: whatever commit it
names is the version on offer. This suits a bare repository on the server that you push to, with no
GitHub credentials on the server: `git init --bare ~/cereal.git` there, `git push <server>:cereal.git main`
from your machine when a version is ready, and `./scripts/updater.sh install ~/cereal.git main`.

## Publishing a version

One version number covers the server and its apps, and each gets it from the one before:

```
 GitHub release v1.2.3 ──▶ server (Update server) ──▶ desktop apps (they ask their server)
   tag + Mac build            builds the tag,            offered the Mac build the server
                              fetches the Mac build      now holds; they never ask GitHub
```

1. Set the version everywhere (`bun scripts/check-version.ts` says where), commit and push.
2. Build the Mac app from that commit: `./scripts/release-macos.sh`.
3. `./scripts/release-github.sh` makes the release `v<version>` on that commit and uploads the Mac build's
   two files to it. It refuses a version that was released before.
4. On the server: **Instance settings → Updates → Update server**. While it builds the new version the
   updater downloads the release's Mac build into the new release's downloads, so the updated server offers
   it. Desktop apps see it the next time they check and update themselves from their server.

A release without a Mac build (`--no-mac`) updates servers only. Downloading the Mac build needs the
repository to be public on GitHub; with a private one, publish the build with
`scripts/deploy-ssh.sh --release` as before. The iPhone app comes from TestFlight or the App Store; like the
desktop app it only says an update exists when it is behind its server, never when it is ahead.

## Enable it on an existing deployment (once)

1. Deploy a version that contains the updater the usual way (`./scripts/deploy-ssh.sh <user@host> main`).
   That brings `scripts/updater.sh` and the `compose.yaml` that mounts its folder.
2. On the server, in the deployment folder (`~/cereal`), run one of the `install` commands above. It writes
   `~/cereal-releases/.updater/config` (private to you), adds `UPDATER_DIR` and `UPDATER_TOKEN` to `.env`,
   tries the source once, and prints the two commands for the next steps.
3. Keep the updater running. With systemd:
   ```
   sudo cp ~/cereal-releases/.updater/cereal-updater.service /etc/systemd/system/
   sudo systemctl daemon-reload && sudo systemctl enable --now cereal-updater
   ```
   It runs as your user, which must be able to run `docker compose` (as it already does for deploys).
4. Restart the server once so the container sees the folder and the token: `cd ~/cereal && docker compose up -d`.
5. Open the admin panel → Updates.

From the shell, without the panel: `./scripts/updater.sh status`, and with the service stopped,
`./scripts/updater.sh check` and `./scripts/updater.sh update`.

## When an update fails

- **Before the switch** (fetch, checks, backup, build): nothing changed; the page says which step failed.
- **After the switch** (the new version does not start or never reports healthy): the previous release is
  started again from its own image and the page says "rolled back". If the failed version had already
  migrated the database, restore the dump taken before it; the log names the file, and
  `scripts/deploy-ssh.sh` prints the restore commands.
- **The previous release does not come back either:** the page cannot say so, because there is no server.
  The job log is `~/cereal-releases/.jobs/<release>/log` on the machine.

## Limits

- One machine, Docker Compose, the layout `scripts/deploy-ssh.sh` makes. The first update of a deployment
  that is still a plain folder moves it into `~/cereal-releases`, as a deploy over SSH does.
- The updater's progress files (`spool/state`, `job`, `log`) are readable by other users of the machine,
  because the container runs as a different user. They hold versions, paths and log lines, never a secret.
- An update restarts the server: live transcription stops until each recording app reconnects.
- The Mac build comes with an update only when the release carries one and the repository is public on
  GitHub. Otherwise an update keeps the builds that are already published, and a new one is published with
  `scripts/deploy-ssh.sh --release` or `scripts/publish-to-docker.sh`.
