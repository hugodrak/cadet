# Exports the model as STL and opens it in an installed slicer.
import cadet

SLICERS = ["OrcaSlicer", "PrusaSlicer", "BambuStudio", "UltiMaker Cura", "Cura"]


@cadet.command("Send to slicer")
def send_to_slicer(slicer="auto"):
    path = cadet.temp_path("cadet-part.stl")
    cadet.export_stl(path)
    app = None if slicer == "auto" else slicer
    if app is None and cadet.PLATFORM == "macos":
        app = next((s for s in SLICERS if cadet.exists("/Applications/%s.app" % s)), None)
    cadet.open_file(path, app)
    print("Sent", path, "to", app or "the default app for STL files")
