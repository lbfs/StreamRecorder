import time
import os
import json
from enum import Enum
from datetime import datetime

import asyncio
import aiohttp
import multidict

from twitch import TwitchHelixAPI

class RecorderConfiguration:
    def __init__(self, filename):
        self.filename = filename
        self.do_update()

    @property
    def needs_update(self):
        return os.path.getmtime(self.filename) != self.last_modified

    def do_update(self):
        with open(self.filename) as config_file:
            config = json.load(config_file)
        
        self.client_id = config["client_id"]
        self.client_secret = config["client_secret"]
        self.recording_path = config["recording_path"]
        self.processed_path = config["processed_path"]
        self.users = config["users"]
        self.last_modified = os.path.getmtime(self.filename)


class Recorder:
    @staticmethod
    async def recording_task(recording_path, stream):
        recording_path = os.path.join(recording_path, stream.user_name)
        if not os.path.isdir(recording_path):
            os.makedirs(recording_path)

        filename = stream.user_name + " - " + str(int(time.time())) + " - " + stream.title + ".mp4"
        filename = "".join(x for x in filename if x.isalnum() or x in [" ", "-", "_", "."])
        recorded_filename = os.path.join(recording_path, filename)
        command = ["streamlink", "--twitch-disable-hosting", stream.url, "best", "-o", recorded_filename]
        process = await asyncio.create_subprocess_exec(*command, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
        await process.wait()

        return filename, recorded_filename, stream

    @staticmethod
    async def cleanup_task(processed_path, queue):
        while True:
            filename, recorded_filename, stream = await queue.get()

            output_path = os.path.join(processed_path, stream.user_name)
            if not os.path.isdir(output_path):
                os.makedirs(output_path)

            export_filename = os.path.join(output_path, filename)
            command = ['ffmpeg', '-nostdin', '-y', '-err_detect', 'ignore_err', '-i', recorded_filename, '-c', 'copy', export_filename]
            process = await asyncio.create_subprocess_exec(*command, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
            await process.wait()
            os.remove(recorded_filename)

    @staticmethod
    async def main(loop, config: RecorderConfiguration):
        api = await TwitchHelixAPI.build(config.client_id, config.client_secret)
        ids = await api.get_user_id_by_login(config.users)

        tasks = {}
        completed_ids = []
        cleanup_queue = asyncio.Queue()
        cleanup_task = loop.create_task(Recorder.cleanup_task(config.processed_path, cleanup_queue))

        while True:
            for key in tasks.keys():
                try:
                    result = tasks[key][1].result()
                    completed_ids.append(key)
                    print(f"{tasks[key][0].user_name} has gone offline. Recording stopped.")
                    await cleanup_queue.put(result)
                except Exception as e:
                    continue
            
            for key in completed_ids:
                del tasks[key]
            completed_ids.clear()

            if config.needs_update:
                await api.teardown()
                config.do_update()
                api = await TwitchHelixAPI.build(config.client_id, config.client_secret)
                ids = await api.get_user_id_by_login(config.users)
                print("Configuration has been reloaded.")

            streams = await api.get_streams_by_user_id(ids)
            for stream in streams:
                if stream.user_id in tasks.keys():
                    continue

                print(f"{stream.user_name} has gone live. Starting recording.")
                tasks[stream.user_id] = (stream, loop.create_task(Recorder.recording_task(config.recording_path, stream)))
            
            await asyncio.sleep(15)

        await api.teardown()


if __name__ == "__main__":
    loop = asyncio.ProactorEventLoop()
    loop.run_until_complete(Recorder.main(loop, RecorderConfiguration("config.json")))