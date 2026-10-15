# The GB Emulator Shootout's harness class for Nubo: the draft of its
# `emulators/nubo.py` (docs/SHOOTOUT_PLAN.md, Part 2), which the pull request
# to the Shootout copies. `.github/workflows/shootout.yml` puts it in a
# checkout of the Shootout and runs the Shootout's tests on the Windows zip.
from util import *
from emulator import Emulator
from test import *
import os


class Nubo(Emulator):
    def __init__(self):
        super().__init__("Nubo", "https://github.com/mschoenm/nubo", startup_time=2.5, features=(PCM,))
        # While a game runs the title is "<game> — Nubo".
        self.title_check = lambda title: title.endswith("Nubo")
        self.speed = 4.0

    def setup(self):
        # The release also has the MSI and the RetroArch core's zip, which a looser filter
        # would pick.
        downloadGithubRelease("mschoenm/nubo", "downloads/nubo.zip",
                              filter=lambda name: name == "Nubo-windows-x64.zip", require_asset=True)
        extract("downloads/nubo.zip", "emu/nubo")
        setDPIScaling("emu/nubo/Nubo/nubo.exe")

    def startProcess(self, rom, *, model, required_features):
        model = {DMG: "dmg", CGB: "cgb", SGB: "sgb"}.get(model)
        if model is None:
            return None
        return subprocess.Popen([
            # Windows doesn't find a relative program path once cwd is set.
            os.path.abspath("emu/nubo/Nubo/nubo.exe"), os.path.abspath(rom),
            "--model", model, "--scale", "1", "--fresh", "--speed", "4",
            # The emulator's own picture: plain grays, and no color correction or
            # frame blending, which players get by default.
            "--raw-screen",
        ], cwd="emu/nubo/Nubo")

    def getScreenshot(self):
        screenshot = getScreenshot(self.title_check)
        if screenshot is not None and screenshot.size == (256, 224):
            # A Super Game Boy shows the Game Boy screen inside its border.
            return screenshot.crop((48, 40, 48 + 160, 40 + 144))
        return screenshot
