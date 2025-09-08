# Introduction

Any time a person uses a computer to access information over the Worldwide Web, buy something from an online vendor, or perform some sort of productivity task (such as writing a document, using a shared calendar or creating a business document), they will, mostly likely, perform that task using a Web Browser.

However, the machine on which the Web Browser (the client) runs is frequently located at a large physical distance from the Web Server with which they are interacting.
This in turns means that the data involved in each request/response cycle must travel through a potentially large number of network switches, routers and servers before completing its round trip.

It is therefore self-evident that the fewer intermediate steps there are in this "_journey through the network_", the quicker the request/response cycle can be completed.

In simplistic terms, the request/response cycle between a Web Browser and a Web Server looks something like this:

```mermaid
sequenceDiagram
    participant Browser
    participant Web Server
    
    Note over Browser, Web Server: User enters a website address
    Browser->>Web Server: Hey website, send me your first page
    Web Server-->>Browser: OK, here you go
    
    Note over Browser, Web Server: Browser examines page content<br>and makes further requests
    Browser->>Web Server: So I also need some image files, some style<br>sheets and a few JavaScript programs
    Web Server-->>Browser: Ok, here are the images
    Web Server-->>Browser: and the style sheets
    Web Server-->>Browser: and the JavaScript...
    
    Note over Browser, Web Server: User interacts with the web page
    Browser->>Web Server: Now I need another web page with<br>some more images and stylesheets
    Web Server-->>Browser: Ok, here are the images
    Web Server-->>Browser: Hang on, didn't I just send you that style sheet? 
```

Whilst all browsers operate their own local cache to avoid requesting a resource they have already been sent, it is not always possible for a browser to recognise that it is requesting the same resource.

This might be because the URL pointing to a particular resource might use a dynamically generated path (or even file) name which changes between visits or between user sessions: yet the actual resource behind the request remains the same.