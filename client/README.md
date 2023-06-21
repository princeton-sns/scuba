# NoiseKV Design Thoughts/Decisions

## Linked devices and Contacts

Initially, the notion of contacts was baked into the data abstraction libraries
because it was a common-enough application feature that it was easier to implement
once for all applications. However, embedding contacts in the data abstraction
library meant that it was more difficult for contacts to leverage the existing
groups/permissions mechanisms, since it was treated as a "special" instance of
such structures. This not only makes contacts less flexible (e.g. limiting sharing
to contacts imposes additional constraints on the overall policy structure) but
also misses the opportunity for employing economy of mechanism, since we have two
separate code paths (contacts vs all other groups) that kind of do the same thing.

I am now in the process of trying to remove the notion of contacts from noise-kv,
and instead set contacts up as any other group would be set up from the application
end. What makes this interesting is the unique access control structure that contacts 
have.

### Permissions

First, let me summarize the permissions structure currently implemented
by the library: "readers" are devices on which the groups + data are stored, "writers"
are devices that can modify data, and "admins" are devices that can modify groups.
"writers" also contain "reader" privileges, and "admins" also contain "writer" 
privileges. This choice was made to simplify the implementation, but precludes
permissions levels that are not strict supersets of each other, e.g. having the
permissions to modify groups without the permission to modify data. This is not
a fundamental limitation, however, and can be implemented in a different version 
of the data abstraction layer.

### Linked groups

Before jumping into contacts, let's briefly go over what a user's linked device
grouping looks like, and how it changes. This ideally will be very similar to
contact data.

The top-level group ("devices") contains pointers to every distinct "linked" subgroup
the device has access to (including its own). Say the current device's linked 
subgroup has id "A", and there are two devices: "A1" and "A2". Say the current
device is "A1". The "devices" group will point to group "A", which will point to 
both device "A1" and "A2". "A" has "admin" access to "devices", "A", "A1", and 
"A2", meaning that each individual device has "admin" access to all of "devices",
"A", "A1", and "A2".

TODO does every node need to have its own permissions object? Or can permissions
trickle down the tree? Or is there a mix -> trickling down unless one is specified
at a particular point? If trickling down or mixing, only one permissions object is 
needed at this point (w - ["A"], r - ["A"]). Otherwise, three identical permissions
objects are needed for each node in the current tree.

If the user associated with "A" wants to add a third device, it must do so via
in existing device, i.e. either "A1" or "A2". The third device, let's assign it
id "A3", asks for permission to be added by one of the existing devices, let's say 
"A1" (via, e.g., a QR code). If "A1" rejects it, nothing changes. 

If "A1" accepts it, it should act as any other group membership change (TODO remove 
"linked group" notion from library as well, but only after contacts). Currently, 
"A1" will send "A3" the existing linked group info so its state is synced with "A1"
and "A2", and then "A1" adds "A3" as a child of "A" (which gets propagated to "A1",
"A2", and "A3"). Then "A1" updates the permissions set to also include "A3". All 
of these operations are valid since "A1" has "admin" permissions for group "A".

### Contacts

Ok, now let's talk about contacts. Say another distinct set of linked devices has
id "B", with individual devices being "B1" and "B2". The top-level "devices" group
on each of these two devices points to "B", which then points to both "B1" and
"B2". Say "B" wants to add "A" as a contact. For simplicity, let's assume that
the contact relationship is bidirectional, i.e. if "B" adds "A" as a contact then
"A" will also have "B" as a contact. This isn't so much "contacts" as we know
them on our phones, but more of a mutual identification/discovery mechanism such
that operations can be properly encrypted and routed to the intended device(s).

TODO need to better understand contact discovery/how one "finds" new contacts.

Assuming "B1" has already "found" "A1" (where the ids of the devices double as 
public keys), "B1" can send a contact request to "A1" (in the implementation, "B"'s
device group is piggybacked along with this request). If "A1" rejects the request,
nothing changes.

If "A1" accepts the request, it first adds "B"'s subgroup to its top-level "devices" 
group, and then sends its own subgroup back to "B1". The goal is that each "linked"
subgroup will be kept in sync with its originator should its linked group membership
change. For instance, if "B3" becomes added to "B"'s subgroup by "B1" (as "A3"
was added by "A1" above), that membership change will be propagated to all of
"A"'s devices in addition to all of "B"'s devices, such that any future operations
between the two linked groups go to all linked devices. Similarly, if the user 
associated with group "B" chooses to unlink "B2", operations between "A" and "B" 
should no longer go to the device previously identified as "B2".

Now that we've established the goal and the high-level process of adding a contact,
let's peer into the details and reason about how the associated permissions of
each group mitigate malicious behavior between devices that do not have a contact
relationship.

When "A1" adds "B"'s subgroup to its top-level "devices" group, this happens in
several steps. "A1" first adds, as a child to "devices", id "B" (this can also happen
after the next step, and may in fact be better after the next step so there isn't 
a dangling pointer although these two steps should happen atomically). Recall that 
"A1" has "admin" permissions on the "devices" group, so it can modify its structure.
This change will propagate to all of "A"'s devices (since they are all admins and
thus readers on the "devices" group).

Then, since "A1" has "B"'s subgroup (recall it was piggybacked with the request),
"A1" should store "B"'s subgroup. Before sending the initial contact request, however,
"B" should have given "A" read permissions to its subtree such that any changes
to it are propagated. Adding "A" as a reader should also trigger the initial propagation
such that a special "add-contact" method doesn't need to exist (anymore). Similarly,
once "A" accepts the request, it should add "B" as a reader to its linked group,
which sends "A"s linked group to "B" and also any future updates to it.

However, imagine now that another user "C" wants to add "A" as a contact. By adding
"C" as a reader to "A"s linked group, "C" may be able to see that "A" has shared
its information with "B", and "B" may see the same about "C". This is where things
start to look a bit different from a "normal" group. A new permission type may
fix this, where the list of readers is not propagated but everything else is.




