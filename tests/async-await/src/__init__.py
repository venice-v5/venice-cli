from vasyncio import Sleep
import vasyncio

async def sleep(ms):
    await Sleep(millis=ms)
    print(f"slept {ms} ms")
    return 100

async def main():
    task = vasyncio.spawn(sleep(500))
    await sleep(1000)
    ret = await task
    print(f"task returned {ret}")

vasyncio.run(main())
