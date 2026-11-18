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

The source is a private repository, so the server needs a git remote it can read. Two ways:

1. **A read-only deploy key.** Add a deploy key (read access only) to the repository on GitHub, put the
   private key on the server, and install with
   `./scripts/updater.sh install git@github.com:<you>/cereal.git main --ssh-key ~/.ssh/cereal_deploy_key`.
2. **A bare repository on the server that you push to.** Nothing on the server holds GitHub credentials:
   `git init --bare ~/cereal.git` on the server, `git push <server>:cereal.git main` from your machine when
   a version is ready, and install with `./scripts/updater.sh install ~/cereal.git main`.

`main` can be any branch or tag. **"Newer" means: the commit that ref names is not the commit this server
runs.** The version number shown is `version` in that commit's `package.json`. A commit that is behind the
running one is never installed (migrations only go forward). The updater looks when the owner presses
**Check for updates**, when it starts, and every six hours.

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
- The updater installs what the configured ref points to. Publishing a Mac build is still
  `scripts/deploy-ssh.sh --release` or `scripts/publish-to-docker.sh`; an update keeps the builds that are
  already published.
