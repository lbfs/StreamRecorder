
import time
import os
from enum import Enum
from datetime import datetime

import asyncio
import aiohttp
import multidict

class TwitchStreamStatus(Enum):
    OFFLINE = 0
    LIVE = 1

class TwitchStream:
    def __init__(self):
        self.user_id = None
        self.user_name = None
        self.started_at = None
        self.url = None
        self.status = TwitchStreamStatus.OFFLINE
        self.stream_id = None
        self.title = None
        self.game_id = None

class TwitchHelixAPI:
    API_BASE_URL = "https://api.twitch.tv/helix/"
    API_TOKEN_URL = "https://id.twitch.tv/oauth2/token"

    def __init__(self):
        self.headers = None
        self.client_id = None
        self.client_secret = None
        self.session = None
        self.ratelimiter = None
        self.ratelimiter_remaining = None
        self.ratelimiter_reset = None

    @staticmethod
    async def build(client_id, client_secret):
        if not isinstance(client_id, str):
            raise TypeError("build: Improper client identifier passed, must be str.")

        if not isinstance(client_secret, str):
            raise TypeError("build: Improper client identifier passed, must be str.")

        api = TwitchHelixAPI()
        api.client_id = client_id
        api.client_secret = client_secret
        api.headers = {"Client-ID": api.client_id}
        api.session = aiohttp.ClientSession()
        api.ratelimiter = asyncio.Semaphore()
        await api.request_access_token()
        return api

    async def request_access_token(self):
        params = {
            "client_id": self.client_id,
            "client_secret": self.client_secret,
            "grant_type": "client_credentials"
        }

        async with self.session.post(self.API_TOKEN_URL, params=params) as response:
            data = await response.json()
            access_token = data["access_token"]
            self.headers["Authorization"] = f"Bearer {access_token}"

    async def get(self, path, params):
        async with self.ratelimiter:
            if self.ratelimiter_remaining == 0:
                reset = int(self.ratelimiter_reset) - int(time.time())
                if reset > 0:
                    await asyncio.sleep(reset)
            return await self._get(path, params)

    async def _get(self, path, params, error_retries=0):
        if error_retries > 5:
            raise Exception(f"Max retry limit exceeded, unable to process request with {path} and {params}.")

        async with self.session.get(self.API_BASE_URL + path, params=params, headers=self.headers) as response:
            if int(response.status) == 401:
                await self.request_access_token()
                await asyncio.sleep(error_retries * 5)
                return await self._get(path, params, error_retries=error_retries + 1)

            data = await response.json()
            self.ratelimiter_remaining = int(response.headers["Ratelimit-Remaining"])
            self.ratelimiter_reset = int(response.headers["Ratelimit-Reset"])
            return data

    async def get_user_id_by_login(self, login):
        if isinstance(login, list):
            items = []
            for item in login:
                if not isinstance(item, str):
                    raise TypeError("get_user_id_by_login: Improper login passed to function, must be str or list of str.")
                items.append(("login", item))
            items = multidict.MultiDict(items)
        elif isinstance(login, str):
            items = {"login": login}
        else:
            raise TypeError("get_user_id_by_login: Improper login passed to function, must be str or list of str.")

        response = await self.get("users", items)
        return [int(user["id"]) for user in response["data"]]

    async def get_streams_by_user_id(self, user_id):
        if isinstance(user_id, list):
            items = []
            for item in user_id:
                if not isinstance(item, int):
                    raise TypeError("get_streams_by_user_id: Improper user_id passed to function, must be int or list of integers.")
                items.append(("user_id", item))
            items = multidict.MultiDict(items)
        elif isinstance(user_id, int):
            items = {"user_id": user_id}
        else:
            raise TypeError("get_streams_by_user_id: Improper user_id passed to function, must be int or list of integers.")

        response = await self.get("streams", items)
        return [self.convert_stream_response_to_twitch_stream(stream) for stream in response["data"]]

    @staticmethod
    def convert_stream_response_to_twitch_stream(response):
        stream = TwitchStream()
        stream.title = response["title"]
        stream.user_id = int(response["user_id"])
        stream.user_name = response["user_name"]
        stream.game_id = int(response["game_id"])
        stream.stream_id = int(response["id"])
        stream.url = "https://www.twitch.tv/" + response["user_name"]
        stream.started_at = datetime.strptime(response["started_at"], '%Y-%m-%dT%H:%M:%SZ')
        stream.status = TwitchStreamStatus.LIVE if response["type"] == "live" else TwitchStreamStatus.OFFLINE
        return stream

    async def teardown(self):
        await self.session.close()

