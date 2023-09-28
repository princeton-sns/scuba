# Taking stock of how contacts are currently broken and any temporary hacks around it

## Status (using smart lightbulb)

Using `is_contact_name` flag in group to identify top-level contact names

Initial `add-contact` works

The two contact pointers (one on each device) correctly point to each other

Can share data with added contact

Immediately after sharing data, the sharer's contacts are fine but sharee's
contacts (`is_contact_name` field) are overriden; sharer sends own version of
some subgroups during sharing, so sharer's contact info starts to leak to the
sharee

At this point, the added contact (with write privs) can still successfully modify 
shared data, as can the data owner/sharer

But now the `get_contacts` command on the sharee returns the empty list
(once it returned its own device name, but cannot yet reproduce this)

However, nothing seems to check whether a group is a contact or not before 
sharing, so for the purpose of sketching out applications this is fine for 
the time being (contacts, as tracked by noise, are messed up, but this shouldn't
inhibit other functionality)
