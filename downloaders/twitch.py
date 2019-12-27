import asyncio
import multidict
import time
from recorder import Recorder

import logging
logger = logging.getLogger("__main__")

class TwitchHelixAPI:
    def __init__(self, session, client_id, client_secret=None):
        """ Creates a connection to the Twitch Helix API, Requires an AIOHTTP Client Session """
        self.CLIENT_ID = client_id
        self.CLIENT_SECRET = client_secret
        self.API_BASE_URL = "https://api.twitch.tv/helix/"
        self.API_TOKEN_URL = "https://id.twitch.tv/oauth2/token"
        self.headers = {"Client-ID": self.CLIENT_ID}
        self.session = session
        self.ratelimiter = asyncio.Semaphore()
        self.ratelimiter_remaining = None
        self.ratelimiter_reset = None

    @staticmethod
    async def build(session, client_id, client_secret=None):
        """ Builds a TwitchHelixAPI when using a client_id and optional client_secret. Use instead of the constructor. """
        api = TwitchHelixAPI(session, client_id, client_secret)
        if client_secret is not None:
            await api.request_access_token() #TODO: Add Error Checking on Failure
        return api

    async def request_access_token(self):
        """ Use client_secret when performing requests to allow larger bucket size """
        params = {
            "client_id": self.CLIENT_ID,
            "client_secret": self.CLIENT_SECRET,
            "grant_type": "client_credentials"
        }
        async with self.session.post(self.API_TOKEN_URL, params=params) as response:
            data = await response.json()
            access_token = data["access_token"]
            self.headers["Authorization"] = f"Bearer {access_token}"

    async def get(self, path, params):
        """ Used for performing requests on the API with Ratelimiting """
        async with self.ratelimiter:
            if self.ratelimiter_remaining == 0:
                reset = int(self.ratelimiter_reset) - int(time.time())
                reset = 0 if reset < 0 else reset
                await asyncio.sleep(reset)
            return await self._get(path, params)

    async def _get(self, path, params, error_retries=0, error_backoff=2, error_backoff_exponent=2, max_retries=5):
        """  Used for performing requests on the API without Ratelimiting. Should not be used by outside components. """
        try:
            async with self.session.get(self.API_BASE_URL + path, params=params, headers=self.headers) as response:
                if int(response.status) == 401 and self.CLIENT_SECRET is not None:
                    await self.request_access_token() #TODO: Better Error Handling
                    return await self._get(path, params)
                data = await response.json()
                self.ratelimiter_remaining = int(response.headers["Ratelimit-Remaining"])
                self.ratelimiter_reset = int(response.headers["Ratelimit-Reset"])
                return data
        except Exception as e:
            logger.warning(f"Exception encountered during get request: {e}")
            if error_retries < max_retries:
                error_retries = error_retries + 1
                await asyncio.sleep(error_backoff)
                error_backoff = error_backoff ** error_backoff_exponent
                return await self._get(path, params, error_retries, error_backoff)
            else:
                logger.warning("Failed to get proper response, returning empty API response")
                return {"data":[],"pagination":{}}

    async def convert_login_to_id(self, username):
        """ Converts a login name to an id. Returns None if user does not exist. """
        params = { "login": username }
        data = await self.get("users", params)
        try:
            user_id = data["data"][0]["id"]
        except KeyError:
            user_id = None
        return user_id

    async def get_stream_by_id(self, user_id):
        """ Return current stream information for user by id """
        if isinstance(user_id, list):
            items = []
            for item in user_id:
                items.append(("user_id", item))
            items = multidict.MultiDict(items)
        else:
            items = {"user_id": user_id}
        return await self.get("streams", items)

    async def get_stream_by_login(self, user_login):
        """ Return current stream information for user by name """
        if isinstance(user_login, list):
            items = []
            for item in user_login:
                items.append(("user_login", item))
            items = multidict.MultiDict(items)
        else:
            items = {"user_login": user_login}
        return await self.get("streams", items)

    async def get_videos(self, user_id):
        """ Request all videos from a channel by user_id """
        parameters = {"user_id": user_id}
        while True:
            data = await self.get("videos", parameters)
            if "error" in data:
                break
            for entry in data["data"]:
                yield entry
            if "cursor" not in data["pagination"]:
                break
            parameters["after"] = data["pagination"]["cursor"]

class TwitchRecorder(Recorder):
    def __init__(self, api, streamlink_oauth_token=None, **kwargs):
        super().__init__(**kwargs)
        self.api = api

        if streamlink_oauth_token:
            self.streamlink_options = ["--twitch-disable-hosting", "--twitch-oauth-token", streamlink_oauth_token]
        else:
            self.streamlink_options = ["--twitch-disable-hosting"]
        
        self.semaphore = asyncio.Semaphore()
        self.previous_time = 0
        self.tasks = dict()
        self.stream = None

    async def check_stream_function(self, username):
        async with self.semaphore:
            if time.time() - self.previous_time > self.refresh_rate or self.stream is None:
                self.stream = await self.api.get_stream_by_login(list(self.tasks.keys())) #TODO: Fix Bug When > 100 User Logins API Limitation
                self.previous_time = time.time()
        
        for entry in self.stream.get("data", []):
            response_username = entry.get("user_name", None)
            if response_username.lower() == username.lower():
                response_status = entry.get("type", None)
                if response_status == "live":
                    return True, entry["title"]
                else:
                    return False, None
        
        return False, None

    async def register_channel(self, username):
        if username not in self.tasks:
            self.tasks[username] = asyncio.create_task(self.record(username, f"https://www.twitch.tv/{username}", self.check_stream_function))
            return self.tasks[username]

    async def unregister_channel(self, username):
        if username in self.tasks:
            self.tasks[username].cancel()