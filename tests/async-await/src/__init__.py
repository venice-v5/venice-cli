import vasyncio
from vasyncio import Sleep

async def sleep():
    await vasyncio.sleep_ms(1000)

async def main():
    print("sleeping")
    await Sleep(millis=1000)
    print("slept for 1000 ms")

event_loop = vasyncio.new_event_loop()
event_loop.spawn(main())
event_loop.run()
