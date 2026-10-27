# Involute spur gear (20° pressure angle). Shows how a plugin builds geometry.
import math
import cadet


def involute(a):
    return math.tan(a) - a


@cadet.command("Spur gear")
def spur_gear(teeth=20, module=2.0, thickness=6.0, bore=5.0, steps=6):
    rp = module * teeth / 2
    rb = rp * math.cos(math.radians(20))
    ra = rp + module
    rf = max(rp - 1.25 * module, 0.1)
    half = math.pi / (2 * teeth) + involute(math.acos(rb / rp))
    start = max(rb, rf)

    def flank_angle(r):
        return half - involute(math.acos(min(rb / r, 1.0)))

    def at(r, a):
        return (r * math.cos(a), r * math.sin(a))

    points = []
    for i in range(int(teeth)):
        c = 2 * math.pi * i / teeth
        if rf < rb:
            points.append(at(rf, c - half))
        radii = [start + (ra - start) * k / steps for k in range(int(steps) + 1)]
        points += [at(r, c - flank_angle(r)) for r in radii]
        points += [at(r, c + flank_angle(r)) for r in reversed(radii)]
        if rf < rb:
            points.append(at(rf, c + half))

    s = cadet.sketch("XY")
    cadet.polygon(s, points)
    if bore > 0:
        cadet.circle(s, 0, 0, bore / 2)
    cadet.extrude(s, thickness)
    cadet.run("view.home")
    print(f"Gear: {teeth} teeth, module {module}, pitch Ø {2 * rp:.3f} mm, outside Ø {2 * ra:.3f} mm")
