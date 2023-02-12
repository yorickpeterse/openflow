# OpenFlow

OpenFlow is a ventilation system built around Itho Daalderop's
[DemandFlow/QualityFlow](https://www.ithodaalderop.nl/nl-NL/professional/productoverzicht/a04_03_01)
ventilation system.

In a nutshell, the DF/QF system works as follows: each room has an exhaust duct
connected to a central box up in the attic (the "plenum"). A ventilation unit
exhausts air through these ducts into the plenum and to the outside of the
house. Fresh air is pumped into the house in the staircase at the highest point
(typically close to the ventilation system). Instead of one CO2 sensor for every
room, the system uses a centralised CO2 sensor that sits in the plenum. To
determine which rooms need ventilating, the system continuously samples the CO2
levels, adjusting ventilation accordingly.

While this setup sounds interesting on paper, in practise it leaves a lot to be
desired. Most notably, the use of a single centralised CO2 sensor results in
skewed CO2 levels, which in turn results in the system ventilating the wrong
rooms or not ventilating them on time. In particular I found that if CO2 levels
rise in one room, the system has a tendency to think the levels are also
increasing in other rooms, even if those rooms haven't been used for hours. As
the ventilation speed is based on the CO2 levels, this also leads to higher
exhaust speeds than should be necessary, and thus more noise.

Enter "OpenFlow": a program written in [Inko](https://inko-lang.org/) that takes
over control of the DF/QF system in an attempt to work around these issues.

Instead of sampling rooms, OpenFlow uses [Philips Hue motion
sensors](https://www.philips-hue.com/en-us/p/hue-motion-sensor/046677570972) to
determine what rooms are in need of ventilation, only using the centralised CO2
sensor to further increase the ventilation speed. Ventilation in turn is kept
active based on different sources, such as continued motion/presence, IPs
responding to ping requests (so you can for example keep ventilating the living
room while the TV is on), or the room's humidity.

In addition rooms can be ventilated at night, regardless of any motion being
detected. This is useful for bedrooms, as the DF/QF system has a tendency to not
ventilate these nearly as much as desired.

OpenFlow focuses on ventilating earlier at lower noise levels, rather than
waiting until some CO2 threshold is crossed and then ventilating more
aggressively. OpenFlow also tries to be more clever about when to stop
ventilating, ventilating humid rooms _without_ running the exhaust fan at 90%
for a long time, and in general being more reliable and efficient.

# Requirements

- An Itho Daalderop DemandFlow/QualityFlow ventilation unit, paired with e.g. a
  [HRU ECO 350](https://www.ithodaalderop.nl/nl-NL/consument/productoverzicht/a04_02_01_03)
- An [Itho HRU/WPU/DemandFlow/QualityFlow wifi
  module](https://www.nrgwatch.nl/product/itho-non-cve-wifi-module/) with [these
  changes applied](https://github.com/arjenhiemstra/ithowifi/pull/144)
- Inko `master`, as we use some changes not yet released
- A bunch of Philips Hue motion sensors, connected to a Philips Hue bridge

# Installation

TODO

# License

All source code in this repository is licensed under the Mozilla Public License
version 2.0, unless stated otherwise. A copy of this license can be found in the
file "LICENSE".
