import time
import os
import json
from enum import Enum
from datetime import datetime

import asyncio
import aiohttp
import multidict

from twitch import TwitchHelixAPI

def clear_console():
    os.system('cls' if os.name == 'nt' else 'clear')

class Recorder:
    def __init__(self, loop, filename):
        self.loop = loop
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

    async def recording_task(self, stream):
        recording_path = os.path.join(self.recording_path, stream.user_name)
        if not os.path.isdir(recording_path):
            os.makedirs(recording_path)

        filename = stream.user_name + " - " + str(int(time.time())) + " - " + stream.title + ".mp4"
        filename = "".join(x for x in filename if x.isalnum() or x in [" ", "-", "_", "."])
        recorded_filename = os.path.join(recording_path, filename)
        command = ["streamlink", "--twitch-disable-hosting", stream.url, "best", "-o", recorded_filename]
        process = await asyncio.create_subprocess_exec(*command, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
        await process.wait()

        return filename, recorded_filename, stream

    async def cleanup_task(self, queue):
        while True:
            filename, recorded_filename, stream = await queue.get()

            output_path = os.path.join(self.processed_path, stream.user_name)
            if not os.path.isdir(output_path):
                os.makedirs(output_path)

            export_filename = os.path.join(output_path, filename)
            command = ['ffmpeg', '-nostdin', '-y', '-err_detect', 'ignore_err', '-i', recorded_filename, '-c', 'copy', export_filename]
            process = await asyncio.create_subprocess_exec(*command, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL)
            await process.wait()

            try:
                os.remove(recorded_filename)
            except Exception as e:
                pass

    async def run(self):
        api = await TwitchHelixAPI.build(self.client_id, self.client_secret)
        ids = await api.get_user_id_by_login(self.users)

        tasks = {}
        completed_ids = []
        cleanup_queue = asyncio.Queue()
        cleanup_task = self.loop.create_task(self.cleanup_task(cleanup_queue))

        while True:
            for key in tasks.keys():
                try:
                    result = tasks[key][1].result()
                    completed_ids.append(key)
                    # print(f"{tasks[key][0].user_name} has gone offline. Recording stopped.")
                    await cleanup_queue.put(result)
                except Exception as e:
                    continue
            
            for key in completed_ids:
                del tasks[key]
            completed_ids.clear()

            if self.needs_update:
                await api.teardown()
                self.do_update()
                api = await TwitchHelixAPI.build(self.client_id, self.client_secret)
                ids = await api.get_user_id_by_login(self.users)
                # print("Configuration has been reloaded.")

            keys = list(tasks.keys())
            clear_console()
            currently_processing = "True" if cleanup_queue.qsize() > 0 else "False"
            current_timestamp = datetime.utcnow().strftime("%Y-%m-%d %H:%M:%S")
            last_loaded_timestamp = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(self.last_modified))
            print(f"Current timestamp: {current_timestamp}\nActive recordings: {len(keys)}")
            if len(keys):
                active_users = ", ".join(keys)
                active_users = f"Active users: {active_users}"
                print(active_users)
            print(f"Currently processing: {currently_processing}\nConfiguration last modified: {last_loaded_timestamp}")

            streams = await api.get_streams_by_user_id(ids)
            for stream in streams:
                if stream.user_id in keys:
                    continue

                # print(f"{stream.user_name} has gone live. Starting recording.")
                tasks[stream.user_id] = (stream, self.loop.create_task(self.recording_task( stream)))
            
            await asyncio.sleep(15)

        await api.teardown()


if __name__ == "__main__":
    loop = asyncio.ProactorEventLoop()
    recorder = Recorder(loop, "config.json")
    loop.run_until_complete(recorder.run())