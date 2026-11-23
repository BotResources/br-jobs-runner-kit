use super::live_server::Server;
use super::live_world::World;

impl World {
    pub async fn delete_stream(&self, name: &str) {
        self.js
            .delete_stream(name)
            .await
            .expect("the stream is deleted");
    }

    pub async fn delete_bucket(&self, name: &str) {
        self.js
            .delete_key_value(name)
            .await
            .expect("the bucket is deleted");
    }
}

impl World {
    pub fn server(&self) -> &Server {
        &self._server
    }

    pub fn server_mut(&mut self) -> &mut Server {
        &mut self._server
    }
}

impl Server {
    pub fn restart(&mut self) {
        self.stop();
        self.spawn();
    }
}
