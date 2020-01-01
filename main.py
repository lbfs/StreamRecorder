#!/usr/bin/env python3

import os
import asyncio
import json

import logging
logging.basicConfig(format='%(asctime)s: %(message)s', datefmt='%m/%d/%Y %I:%M:%S %p')
logger = logging.getLogger("__main__")
logger.setLevel(logging.DEBUG)

import aiohttp
from downloaders.twitch import TwitchHelixAPI, TwitchRecorder

async def main():
    with open("config.json") as config_file:
        config = json.load(config_file)


    CLIENT_ID = config["CLIENT_ID"]
    CLIENT_SECRET = config["CLIENT_SECRET"]
    STREAMLINK_OAUTH_TOKEN = config["STREAMLINK_OAUTH_TOKEN"]
    RECORDING_PATH = config["RECORDING_PATH"]
    PROCESSED_PATH = config["PROCESSED_PATH"]
    CHANNELS_TXT = config["CHANNELS_TXT"]

    while True:
        try:
            async with aiohttp.ClientSession() as session:
                api = await TwitchHelixAPI.build(session, CLIENT_ID, CLIENT_SECRET)
                recorder = TwitchRecorder(api, STREAMLINK_OAUTH_TOKEN, recording_path=RECORDING_PATH, processed_path=PROCESSED_PATH, use_rclone=False)

                channels = []
                last_update_time = 0
                while True:
                    if os.path.getmtime(CHANNELS_TXT) > last_update_time:
                        with open(CHANNELS_TXT) as channel_file:
                            updated_channels = [channel.rstrip() for channel in channel_file]
                        
                        for channel in channels:
                            if channel not in updated_channels:
                                logger.info(f"Unregistering channel by name {channel}")
                                await recorder.unregister_channel(channel)
                        
                        for channel in updated_channels:
                            if channel not in channels:
                                logger.info(f"Registering channel by name {channel}")
                                await recorder.register_channel(channel)
                        
                        channels = updated_channels
                        last_update_time = os.path.getmtime(CHANNELS_TXT)
                    await asyncio.sleep(30)
        except FileNotFoundError:
            logger.critical("Invalid channels.txt provided.")
            break
        except Exception as e:
            logger.critical(str(e))


if __name__ == "__main__":
    loop = asyncio.ProactorEventLoop()
    loop.run_until_complete(main())
