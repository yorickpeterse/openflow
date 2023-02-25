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

If your Itho WiFi add-on has a CC1101 module attached, you can also use the
official Itho Daalderop RF remotes to control ventilation in rooms. This
requires that you set it up per the add-on's documentation, then add it to your
`config.json`; see `config.example.json` for an example.

# Installation

Assuming the WiFi module is set up, you'll need to determine what the setting
indexes are to control the exhaust valves of the rooms you want to ventilate. To
obtain these indexes, go to the "Itho settings" page of the WiFi module and
click "Retrieve settings". The settings you need will be called something along
the lines of "Flap position XXX with manual control" or "Damper position of XXX
with manual control". The indexes are in the "Index" column.

Next you'll need to find the settings indexes that enable the manual control
setting (most likely called "manual control") and the exhaust fan speed
("Ventilation requirement for exhaust air fan in manual mode (%)" or something
along those lines).

Third, you'll need to determine the IP addresses of the WiFi module and the
Philips Hue bridge. OpenFlow doesn't have support for service discovery of any
kind, nor can it resolve host names, so make sure to assign these devices a
static IP address. For the Hue bridge you'll also need a user/API token. You can
find more information on how to do this [on this
page](https://developers.meethue.com/develop/get-started-2/).

With all the information gathered, copy `config.example.json` to
`/etc/openflow.json` and adjust it accordingly.

You can then build and run OpenFlow as follows:

```bash
inko build src/main.inko -o openflow.ibi # Compiles the code
inko run openflow.ibi                    # Runs it
```

To make this process easier, a Docker/Podman container is provided. You can use
it as follows (I'm using `podman` here, but `docker` should also work):

```bash
podman pull registry.gitlab.com/yorickpeterse/openflow/openflow:main
podman run \
    --memory 64m \
    --rm \
    --name openflow \
    --tz=local \
    --volume /etc/openflow.json:/etc/openflow.json \
    --tty \
    --interactive \
    --init \
    openflow:main
```

The `--tz=local` flag ensures the container reuses your system's timezone
instead of using UTC. This is important as otherwise ventilation schedules may
run at a different time from what you'd expect.

The `--init` flag ensures signals are forwarded, that way `Control+C` works as
expected. Without this you'll need to use `podman kill ...` to stop the
container.

If you want to run the container in the background, start it as follows instead:

```bash
podman run \
    --memory 64m \
    --rm \
    --name openflow \
    --tz=local \
    --volume /etc/openflow.json:/etc/openflow.json \
    --detach \
    --init \
    openflow:main
```

# License

All source code in this repository is licensed under the Mozilla Public License
version 2.0, unless stated otherwise. A copy of this license can be found in the
file "LICENSE".
