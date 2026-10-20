"""Checks whether a newer VoxLoom is published. Runs only when the user clicks
Check for updates, never at startup or in the background.

Its one request goes to GitHub's public releases API and carries the app's
version in the User-Agent (GitHub requires one); nothing from the library.
Drafts and pre-releases are never reported. Prints one JSON line: the latest
version and whether it is newer, or an error to show the user.
"""
import argparse
import json
import re
import urllib.error
import urllib.request

LATEST = "https://api.github.com/repos/chencyberlab/VoxLoom/releases/latest"


def version(text):
    match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)", text.strip())
    if not match:
        raise ValueError(text)
    return tuple(int(part) for part in match.groups())


def check(current):
    request = urllib.request.Request(LATEST, headers={"Accept": "application/vnd.github+json",
                                                      "User-Agent": f"VoxLoom/{current}"})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            body = response.read(1_000_000)
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return {"error": "No VoxLoom release has been published yet."}
        if error.code in (403, 429):
            return {"error": "GitHub is busy. Try again in an hour."}
        return {"error": "GitHub could not answer just now. Try again later."}
    except (OSError, ValueError):
        return {"error": "Couldn't reach GitHub. Check your internet connection and try again."}
    try:
        latest = version(str(json.loads(body)["tag_name"]))
    except (KeyError, TypeError, ValueError):
        return {"error": "GitHub's answer could not be read. Try again later."}
    return {"latest": ".".join(map(str, latest)), "newer": latest > version(current)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--current", required=True)
    print(json.dumps(check(parser.parse_args().current)), flush=True)


if __name__ == "__main__":
    main()
