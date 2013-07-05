//
//  PutioMainController.m
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioMainController.h"
#import "AFNetworking.h"
#import "AFJSONRequestOperation.h"
#import "PutioBrowser.h"
#import "SSKeychain.h"

@implementation PutioMainController

@synthesize message, progress, waiting, authWindow, oauthToken, waitingLabel;


-(void)awakeFromNib
{
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:self andSelector:@selector(handleEvent:withReplyEvent:) forEventClass:kInternetEventClass andEventID:kAEGetURL];
    
    [[NSApplication sharedApplication] mainWindow];
    
    [self.waiting startAnimation:nil];
    [self.progress stopAnimation:nil];
    [self authenticateUser];
}


- (void)authenticateUser
{
    NSError *error;
    NSString *t = [SSKeychain passwordForService:@"put.io adder" account:@"711" error:&error];
    
    if ([error code] == SSKeychainErrorNotFound)
    {
        self.message.stringValue = @"Authentication required!";
        [self.waitingLabel setHidden:YES];
        [self.waiting stopAnimation:nil];
        
        self.authWindow = [[PutioBrowser alloc] initWithWindowNibName:@"Browser"];
        [self.authWindow.window makeKeyWindow];
        
        /////////////////////
        // TODO: move auth window in front of *ALL* other windows.
        // Seems simple enough, but I can't get it to work.
        //
        // Pull request, anyone?
        /////////////////////
    }
    else
    {
        self.oauthToken = t;
    }
}


- (void)handleEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent
{
    if (self.oauthToken == nil)
    {
        return [self authenticateUser];
    }
    
    [self.waiting stopAnimation:nil];
    [self.progress startAnimation:nil];
    [self.waitingLabel setHidden:YES];
    
    NSString *magnetURL = [[event paramDescriptorForKeyword:keyDirectObject] stringValue];
    NSError *error = nil;
    
    NSRegularExpression* regex = [NSRegularExpression regularExpressionWithPattern:@"dn=([^&]{5,})" options:0 error:&error];
    NSArray* matches = [regex matchesInString:magnetURL options:0 range:NSMakeRange(0, [magnetURL length])];
    NSString *displayName;
    
    if ([matches count] > 0)
    {
        displayName = [magnetURL substringWithRange:[[matches objectAtIndex:0] rangeAtIndex:1]];
        displayName = [displayName stringByReplacingOccurrencesOfString:@"+" withString:@" "];
        displayName = [displayName stringByReplacingOccurrencesOfString:@"." withString:@" "];
        displayName = [displayName stringByReplacingPercentEscapesUsingEncoding:NSUTF8StringEncoding];
    }
    else
    {
        displayName = magnetURL;
    }
    
    self.message.stringValue = [NSString stringWithFormat:@"Adding: %@", displayName];
    
    NSURL *url = [NSURL URLWithString:@"https://api.put.io/"];
    NSDictionary *params = [NSDictionary dictionaryWithObjectsAndKeys:
        magnetURL, @"url",
        self.oauthToken, @"oauth_token",
    nil];
    
    AFHTTPClient *httpClient = [[AFHTTPClient alloc] initWithBaseURL:url];
    NSMutableURLRequest *request = [httpClient requestWithMethod:@"POST" path:@"/v2/transfers/add" parameters:params];
    AFJSONRequestOperation *operation = [AFJSONRequestOperation JSONRequestOperationWithRequest:request
    success:^(NSURLRequest *request, NSHTTPURLResponse *response, id JSON)
    {
        if ([[JSON valueForKeyPath:@"status"] isEqualToString:@"OK"])
        {
            self.message.stringValue = @"URL successfully added!";
        }
        else
        {
            self.message.stringValue = @"Something went wrong!";
        }

        [self.waiting startAnimation:nil];
        [self.progress stopAnimation:nil];
        [self.waitingLabel setHidden:NO];
    }
    failure:^(NSURLRequest *request, NSHTTPURLResponse *response, NSError *error, id JSON)
    {
        long statusCode = (long)response.statusCode;

        if (statusCode == 400)
        {
            self.message.stringValue = @"Failed! URL already in queue?";
        }
        else
        {
            self.message.stringValue = [NSString stringWithFormat:@"Something went wrong! HTTP error %li!", statusCode];
        }

        [self.waiting startAnimation:nil];
        [self.progress stopAnimation:nil];
        [self.waitingLabel setHidden:NO];
    }];
    
    [operation start];
}

@end