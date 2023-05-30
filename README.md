# Noise Client Libraries

Noise provides a general programming and communication model for applications that fit a client-based architecture. Fitting applications are those in which that amount of data can fit on a single device, computation over data is relatively minimal, and data is shared across a relatively small number of devices. Examples of this are: health-tracking applications (e.g. period trackers), small-scale social media, games (e.g. chess), and IoT applications (e.g. smart light switches). 

This repository consists of the Noise [core client library](https://github.com/princeton-sns/noise-rust/tree/main/client/core), Noise data abstraction layers (providing [sequential](https://github.com/princeton-sns/noise-rust/tree/main/client/sequential-noise-kv) and [serializable](https://github.com/princeton-sns/noise-rust/tree/main/client/serializable-noise-kv) consistency on top of a key-value store), and command-line Noise [applications](https://github.com/princeton-sns/noise-rust/tree/main/apps). 

A compatible Noise server (found [here](https://github.com/princeton-sns/sharded-noise-server)) routes and orders all encrypted operations in the system, enabling offline communication and client validation of a host of consistency models.

The core client library establishes end-to-end encryption, server communication, and mechanisms for detecting consistency violations in a Byzantine setting. Notably, the core library is entirely application-agnostic, and applications can either communicate with it directly or through a relevant data abstraction layer.

The data abstraction layers provide various consistency, data model, and access control abstractions to applications, illustrating how the Noise can support a variety of application needs.

Finally, the applications demonstrate how Noise-based applications can focus on implementing application-specific logic, since cross-device communication, consistency guarantees, access control, and encryption are all handled by the underlying libraries.


